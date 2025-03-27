use crate::account_manager::helpers::account::{
    AccountStatus, ActorAccount, AvailabilityFlags, GetAccountAdminStatusOutput,
};
use crate::account_manager::helpers::auth::{
    AuthHelperError, CreateTokensOpts, RefreshGracePeriodOpts,
};
use crate::account_manager::helpers::invite::CodeDetail;
use crate::account_manager::helpers::password::UpdateUserPasswordOpts;
use crate::account_manager::helpers::repo;
use crate::actor_store::ActorStore;
use crate::auth_verifier::AuthScope;
use crate::db::DbConn;
use crate::models::models::EmailTokenPurpose;
use crate::read_after_write::viewer::{
    Agent, LocalViewer, LocalViewerCreator, LocalViewerCreatorParams,
};
use crate::APP_USER_AGENT;
use anyhow::Result;
use atrium_api::client::AtpServiceClient;
use atrium_xrpc_client::reqwest::ReqwestClientBuilder;
use chrono::offset::Utc as UtcOffset;
use chrono::DateTime;
use futures::try_join;
use helpers::{account, auth, email_token, invite, password};
use libipld::Cid;
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome};
use rocket::Request;
use rsky_common;
use rsky_common::time::{from_micros_to_str, from_str_to_micros, HOUR};
use rsky_common::RFC3339_VARIANT;
use rsky_lexicon::com::atproto::admin::StatusAttr;
use rsky_lexicon::com::atproto::server::{AccountCodes, CreateAppPasswordOutput};
use rsky_oauth::oauth_provider::account::account_store::{
    AccountInfo, AccountStore, SignInCredentials,
};
use rsky_oauth::oauth_provider::client::client_id::ClientId;
use rsky_oauth::oauth_provider::device::device_data::DeviceData;
use rsky_oauth::oauth_provider::device::device_id::DeviceId;
use rsky_oauth::oauth_provider::device::device_store::DeviceStore;
use rsky_oauth::oauth_provider::errors::OAuthError;
use rsky_oauth::oauth_provider::oidc::sub::Sub;
use rsky_oauth::oauth_provider::request::code::Code;
use rsky_oauth::oauth_provider::request::request_data::RequestData;
use rsky_oauth::oauth_provider::request::request_id::RequestId;
use rsky_oauth::oauth_provider::request::request_store::{
    FoundRequestResult, RequestStore, UpdateRequestData,
};
use rsky_oauth::oauth_provider::token::refresh_token::RefreshToken;
use rsky_oauth::oauth_provider::token::token_data::TokenData;
use rsky_oauth::oauth_provider::token::token_id::TokenId;
use rsky_oauth::oauth_provider::token::token_store::{NewTokenData, TokenInfo, TokenStore};
use secp256k1::{Keypair, Secp256k1, SecretKey};
use std::collections::BTreeMap;
use std::env;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;

/// Helps with readability when calling create_account()
pub struct CreateAccountOpts {
    pub did: String,
    pub handle: String,
    pub email: Option<String>,
    pub password: Option<String>,
    pub repo_cid: Cid,
    pub repo_rev: String,
    pub invite_code: Option<String>,
    pub deactivated: Option<bool>,
}

pub struct ConfirmEmailOpts<'em> {
    pub did: &'em String,
    pub token: &'em String,
}

pub struct ResetPasswordOpts {
    pub password: String,
    pub token: String,
}

pub struct UpdateAccountPasswordOpts {
    pub did: String,
    pub password: String,
}

pub struct UpdateEmailOpts {
    pub did: String,
    pub email: String,
}

pub struct DisableInviteCodesOpts {
    pub codes: Vec<String>,
    pub accounts: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct AccountManager {
    pub db: Arc<DbConn>,
}

pub type AccountManagerCreator = Box<dyn Fn(Arc<DbConn>) -> AccountManager + Send + Sync>;

impl AccountManager {
    pub fn new(db: Arc<DbConn>) -> Self {
        Self { db }
    }

    pub fn creator() -> AccountManagerCreator {
        Box::new(move |db: Arc<DbConn>| -> AccountManager { AccountManager::new(db) })
    }

    pub async fn get_account(
        &self,
        handle_or_did: &str,
        flags: Option<AvailabilityFlags>,
    ) -> Result<Option<ActorAccount>> {
        let db = self.db.clone();
        account::get_account(handle_or_did, flags, db.as_ref()).await
    }

    pub async fn get_account_by_email(
        &self,
        email: &str,
        flags: Option<AvailabilityFlags>,
    ) -> Result<Option<ActorAccount>> {
        let db = self.db.clone();
        account::get_account_by_email(email, flags, db.as_ref()).await
    }

    pub async fn is_account_activated(&self, did: &str) -> Result<bool> {
        let account = self
            .get_account(
                did,
                Some(AvailabilityFlags {
                    include_taken_down: None,
                    include_deactivated: Some(true),
                }),
            )
            .await?;
        if let Some(account) = account {
            Ok(account.deactivated_at.is_none())
        } else {
            Ok(false)
        }
    }

    pub async fn get_did_for_actor(
        &self,
        handle_or_did: &str,
        flags: Option<AvailabilityFlags>,
    ) -> Result<Option<String>> {
        match self.get_account(handle_or_did, flags).await {
            Ok(Some(got)) => Ok(Some(got.did)),
            _ => Ok(None),
        }
    }

    pub async fn create_account(&self, opts: CreateAccountOpts) -> Result<(String, String)> {
        let db = self.db.clone();
        let CreateAccountOpts {
            did,
            handle,
            email,
            password,
            repo_cid,
            repo_rev,
            invite_code,
            deactivated,
        } = opts;
        let password_encrypted: Option<String> = match password {
            Some(password) => Some(password::gen_salt_and_hash(password)?),
            None => None,
        };
        // Should be a global var so this only happens once
        let secp = Secp256k1::new();
        let private_key = env::var("PDS_JWT_KEY_K256_PRIVATE_KEY_HEX")?;
        let secret_key =
            SecretKey::from_slice(&Result::unwrap(hex::decode(private_key.as_bytes())))?;
        let jwt_key = Keypair::from_secret_key(&secp, &secret_key);
        let (access_jwt, refresh_jwt) = auth::create_tokens(CreateTokensOpts {
            did: did.clone(),
            jwt_key,
            service_did: env::var("PDS_SERVICE_DID").unwrap(),
            scope: Some(AuthScope::Access),
            jti: None,
            expires_in: None,
        })?;
        let refresh_payload = auth::decode_refresh_token(refresh_jwt.clone(), jwt_key)?;
        let now = rsky_common::now();

        if let Some(invite_code) = invite_code.clone() {
            invite::ensure_invite_is_available(invite_code, db.as_ref()).await?;
        }
        account::register_actor(did.clone(), handle, deactivated, db.as_ref()).await?;
        if let (Some(email), Some(password_encrypted)) = (email, password_encrypted) {
            account::register_account(did.clone(), email, password_encrypted, db.as_ref()).await?;
        }
        invite::record_invite_use(did.clone(), invite_code, now, db.as_ref()).await?;
        auth::store_refresh_token(refresh_payload, None, db.as_ref()).await?;
        repo::update_root(did, repo_cid, repo_rev, db.as_ref()).await?;
        Ok((access_jwt, refresh_jwt))
    }

    pub async fn get_account_admin_status(
        &self,
        did: &str,
    ) -> Result<Option<GetAccountAdminStatusOutput>> {
        let db = self.db.clone();
        account::get_account_admin_status(did, db.as_ref()).await
    }

    pub async fn update_repo_root(&self, did: String, cid: Cid, rev: String) -> Result<()> {
        let db = self.db.clone();
        repo::update_root(did, cid, rev, db.as_ref()).await
    }

    pub async fn delete_account(&self, did: &str) -> Result<()> {
        let db = self.db.clone();
        account::delete_account(did, db.as_ref()).await
    }

    pub async fn takedown_account(&self, did: &str, takedown: StatusAttr) -> Result<()> {
        (_, _) = try_join!(
            account::update_account_takedown_status(did, takedown, self.db.as_ref()),
            auth::revoke_refresh_tokens_by_did(did, self.db.as_ref())
        )?;
        Ok(())
    }

    // @NOTE should always be paired with a sequenceHandle().
    pub async fn update_handle(&self, did: &str, handle: &str) -> Result<()> {
        let db = self.db.clone();
        account::update_handle(did, handle, db.as_ref()).await
    }

    pub async fn deactivate_account(&self, did: &str, delete_after: Option<String>) -> Result<()> {
        account::deactivate_account(did, delete_after, self.db.as_ref()).await
    }

    pub async fn activate_account(&self, did: &str) -> Result<()> {
        let db = self.db.clone();
        account::activate_account(did, db.as_ref()).await
    }

    pub async fn get_account_status(&self, handle_or_did: &str) -> Result<AccountStatus> {
        let got = account::get_account(
            handle_or_did,
            Some(AvailabilityFlags {
                include_deactivated: Some(true),
                include_taken_down: Some(true),
            }),
            self.db.as_ref(),
        )
        .await?;
        let res = account::format_account_status(got);
        match res.active {
            true => Ok(AccountStatus::Active),
            false => Ok(res.status.expect("Account status not properly formatted.")),
        }
    }

    // Auth
    // ----------
    pub async fn create_session(
        &self,
        did: String,
        app_password_name: Option<String>,
    ) -> Result<(String, String)> {
        let db = self.db.clone();
        let secp = Secp256k1::new();
        let private_key = env::var("PDS_JWT_KEY_K256_PRIVATE_KEY_HEX")?;
        let secret_key = SecretKey::from_slice(&hex::decode(private_key.as_bytes())?)?;
        let jwt_key = Keypair::from_secret_key(&secp, &secret_key);
        let scope = if app_password_name.is_none() {
            AuthScope::Access
        } else {
            AuthScope::AppPass
        };
        let (access_jwt, refresh_jwt) = auth::create_tokens(CreateTokensOpts {
            did,
            jwt_key,
            service_did: env::var("PDS_SERVICE_DID").unwrap(),
            scope: Some(scope),
            jti: None,
            expires_in: None,
        })?;
        let refresh_payload = auth::decode_refresh_token(refresh_jwt.clone(), jwt_key)?;
        auth::store_refresh_token(refresh_payload, app_password_name, db.as_ref()).await?;
        Ok((access_jwt, refresh_jwt))
    }

    pub async fn rotate_refresh_token(&self, id: &String) -> Result<Option<(String, String)>> {
        let token = auth::get_refresh_token(id, self.db.as_ref()).await?;
        if let Some(token) = token {
            let system_time = SystemTime::now();
            let dt: DateTime<UtcOffset> = system_time.into();
            let now = format!("{}", dt.format(RFC3339_VARIANT));

            // take the chance to tidy all of a user's expired tokens
            // does not need to be transactional since this is just best-effort
            auth::delete_expired_refresh_tokens(&token.did, now, self.db.as_ref()).await?;

            // Shorten the refresh token lifespan down from its
            // original expiration time to its revocation grace period.
            let prev_expires_at = from_str_to_micros(&token.expires_at);

            const REFRESH_GRACE_MS: i32 = 2 * HOUR;
            let grace_expires_at = dt.timestamp_micros() + REFRESH_GRACE_MS as i64;

            let expires_at = if grace_expires_at < prev_expires_at {
                grace_expires_at
            } else {
                prev_expires_at
            };

            if expires_at <= dt.timestamp_micros() {
                return Ok(None);
            }

            // Determine the next refresh token id: upon refresh token
            // reuse you always receive a refresh token with the same id.
            let next_id = token.next_id.unwrap_or_else(auth::get_refresh_token_id);

            let secp = Secp256k1::new();
            let private_key = env::var("PDS_JWT_KEY_K256_PRIVATE_KEY_HEX").unwrap();
            let secret_key =
                SecretKey::from_slice(&hex::decode(private_key.as_bytes()).unwrap()).unwrap();
            let jwt_key = Keypair::from_secret_key(&secp, &secret_key);

            let (access_jwt, refresh_jwt) = auth::create_tokens(CreateTokensOpts {
                did: token.did,
                jwt_key,
                service_did: env::var("PDS_SERVICE_DID").unwrap(),
                scope: Some(if token.app_password_name.is_none() {
                    AuthScope::Access
                } else {
                    AuthScope::AppPass
                }),
                jti: Some(next_id.clone()),
                expires_in: None,
            })?;
            let refresh_payload = auth::decode_refresh_token(refresh_jwt.clone(), jwt_key)?;
            match try_join!(
                auth::add_refresh_grace_period(
                    RefreshGracePeriodOpts {
                        id: id.clone(),
                        expires_at: from_micros_to_str(expires_at),
                        next_id
                    },
                    self.db.as_ref()
                ),
                auth::store_refresh_token(
                    refresh_payload,
                    token.app_password_name,
                    self.db.as_ref()
                )
            ) {
                Ok(_) => Ok(Some((access_jwt, refresh_jwt))),
                Err(e) => match e.downcast_ref() {
                    Some(AuthHelperError::ConcurrentRefresh) => {
                        Box::pin(self.rotate_refresh_token(id)).await
                    }
                    _ => Err(e),
                },
            }
        } else {
            Ok(None)
        }
    }

    pub async fn revoke_refresh_token(&self, id: String) -> Result<bool> {
        auth::revoke_refresh_token(id, self.db.as_ref()).await
    }

    // Invites
    // ----------

    pub async fn create_invite_codes(
        &self,
        to_create: Vec<AccountCodes>,
        use_count: i32,
    ) -> Result<()> {
        let db = self.db.clone();
        invite::create_invite_codes(to_create, use_count, db.as_ref()).await
    }

    pub async fn create_account_invite_codes(
        &self,
        for_account: &str,
        codes: Vec<String>,
        expected_total: usize,
        disabled: bool,
    ) -> Result<Vec<CodeDetail>> {
        invite::create_account_invite_codes(
            for_account,
            codes,
            expected_total,
            disabled,
            self.db.as_ref(),
        )
        .await
    }

    pub async fn get_account_invite_codes(&self, did: &str) -> Result<Vec<CodeDetail>> {
        let db = self.db.clone();
        invite::get_account_invite_codes(did, db.as_ref()).await
    }

    pub async fn get_invited_by_for_accounts(
        &self,
        dids: Vec<String>,
    ) -> Result<BTreeMap<String, CodeDetail>> {
        let db = self.db.clone();
        invite::get_invited_by_for_accounts(dids, db.as_ref()).await
    }

    pub async fn set_account_invites_disabled(&self, did: &str, disabled: bool) -> Result<()> {
        invite::set_account_invites_disabled(did, disabled, self.db.as_ref()).await
    }

    pub async fn disable_invite_codes(&self, opts: DisableInviteCodesOpts) -> Result<()> {
        invite::disable_invite_codes(opts, self.db.as_ref()).await
    }

    // Passwords
    // ----------

    pub async fn create_app_password(
        &self,
        did: String,
        name: String,
    ) -> Result<CreateAppPasswordOutput> {
        password::create_app_password(did, name, self.db.as_ref()).await
    }

    pub async fn list_app_passwords(&self, did: &str) -> Result<Vec<(String, String)>> {
        password::list_app_passwords(did, self.db.as_ref()).await
    }

    pub async fn verify_account_password(&self, did: &str, password_str: &String) -> Result<bool> {
        let db = self.db.clone();
        password::verify_account_password(did, password_str, db.as_ref()).await
    }

    pub async fn verify_app_password(
        &self,
        did: &str,
        password_str: &str,
    ) -> Result<Option<String>> {
        let db = self.db.clone();
        password::verify_app_password(did, password_str, db.as_ref()).await
    }

    pub async fn reset_password(&self, opts: ResetPasswordOpts) -> Result<()> {
        let db = self.db.clone();
        let did = email_token::assert_valid_token_and_find_did(
            EmailTokenPurpose::ResetPassword,
            &opts.token,
            None,
            db.as_ref(),
        )
        .await?;
        self.update_account_password(UpdateAccountPasswordOpts {
            did,
            password: opts.password,
        })
        .await
    }

    pub async fn update_account_password(&self, opts: UpdateAccountPasswordOpts) -> Result<()> {
        let db = self.db.clone();
        let UpdateAccountPasswordOpts { did, .. } = opts;
        let password_encrypted = password::gen_salt_and_hash(opts.password)?;
        try_join!(
            password::update_user_password(
                UpdateUserPasswordOpts {
                    did: did.clone(),
                    password_encrypted
                },
                self.db.as_ref()
            ),
            email_token::delete_email_token(&did, EmailTokenPurpose::ResetPassword, db.as_ref()),
            auth::revoke_refresh_tokens_by_did(&did, self.db.as_ref())
        )?;
        Ok(())
    }

    pub async fn revoke_app_password(&self, did: String, name: String) -> Result<()> {
        try_join!(
            password::delete_app_password(&did, &name, self.db.as_ref()),
            auth::revoke_app_password_refresh_token(&did, &name, self.db.as_ref())
        )?;
        Ok(())
    }

    // Email Tokens
    // ----------
    pub async fn confirm_email<'em>(&self, opts: ConfirmEmailOpts<'em>) -> Result<()> {
        let db = self.db.clone();
        let ConfirmEmailOpts { did, token } = opts;
        email_token::assert_valid_token(
            did,
            EmailTokenPurpose::ConfirmEmail,
            token,
            None,
            db.as_ref(),
        )
        .await?;
        let now = rsky_common::now();
        try_join!(
            email_token::delete_email_token(did, EmailTokenPurpose::ConfirmEmail, db.as_ref()),
            account::set_email_confirmed_at(did, now, self.db.as_ref())
        )?;
        Ok(())
    }

    pub async fn update_email(&self, opts: UpdateEmailOpts) -> Result<()> {
        let db = self.db.clone();
        let UpdateEmailOpts { did, email } = opts;
        try_join!(
            account::update_email(&did, &email, db.as_ref()),
            email_token::delete_all_email_tokens(&did, db.as_ref())
        )?;
        Ok(())
    }

    pub async fn assert_valid_email_token(
        &self,
        did: &str,
        purpose: EmailTokenPurpose,
        token: &str,
    ) -> Result<()> {
        let db = self.db.clone();
        email_token::assert_valid_token(did, purpose, token, None, db.as_ref()).await
    }

    pub async fn assert_valid_email_token_and_cleanup(
        &self,
        did: &str,
        purpose: EmailTokenPurpose,
        token: &str,
    ) -> Result<()> {
        let db = self.db.clone();
        email_token::assert_valid_token(did, purpose, token, None, db.as_ref()).await?;
        email_token::delete_email_token(did, purpose, db.as_ref()).await
    }

    pub async fn create_email_token(
        &self,
        did: &str,
        purpose: EmailTokenPurpose,
    ) -> Result<String> {
        let db = self.db.clone();
        email_token::create_email_token(did, purpose, db.as_ref()).await
    }
}

impl AccountStore for AccountManager {
    fn authenticate_account(
        &self,
        credentials: SignInCredentials,
        device_id: DeviceId,
    ) -> Option<AccountInfo> {
        todo!()
    }

    fn add_authorized_client(&self, device_id: DeviceId, sub: Sub, client_id: ClientId) {
        todo!()
    }

    fn get_device_account(&self, device_id: &DeviceId, sub: Sub) -> Option<AccountInfo> {
        todo!()
    }

    fn remove_device_account(&self, device_id: DeviceId, sub: Sub) {
        todo!()
    }

    fn list_device_accounts(&self, device_id: &DeviceId) -> Vec<AccountInfo> {
        todo!()
    }
}

impl RequestStore for AccountManager {
    fn create_request(&mut self, id: RequestId, data: RequestData) {
        todo!()
    }

    fn read_request(&self, id: &RequestId) -> Option<&RequestData> {
        todo!()
    }

    fn update_request(&mut self, id: RequestId, data: UpdateRequestData) -> Result<(), OAuthError> {
        todo!()
    }

    fn delete_request(&mut self, id: RequestId) {
        todo!()
    }

    fn find_request_by_code(&self, code: Code) -> Option<FoundRequestResult> {
        todo!()
    }
}

impl DeviceStore for AccountManager {
    fn create_device(&mut self, device_id: DeviceId, data: DeviceData) {
        todo!()
    }

    fn read_device(&self, device_id: DeviceId) -> Option<DeviceData> {
        todo!()
    }

    fn update_device(&mut self, device_id: DeviceId, data: DeviceData) {
        todo!()
    }

    fn delete_device(&mut self, device_id: DeviceId) {
        todo!()
    }
}

impl TokenStore for AccountManager {
    fn create_token(
        &mut self,
        token_id: TokenId,
        data: TokenData,
        refresh_token: Option<RefreshToken>,
    ) -> std::result::Result<(), OAuthError> {
        todo!()
    }

    fn read_token(&self, token_id: TokenId) -> std::result::Result<Option<TokenInfo>, OAuthError> {
        todo!()
    }

    fn delete_token(&mut self, token_id: TokenId) -> std::result::Result<(), OAuthError> {
        todo!()
    }

    fn rotate_token(
        &mut self,
        token_id: TokenId,
        new_token_id: TokenId,
        new_refresh_token: RefreshToken,
        new_data: NewTokenData,
    ) -> std::result::Result<(), OAuthError> {
        todo!()
    }

    fn find_token_by_refresh_token(
        &self,
        refresh_token: RefreshToken,
    ) -> std::result::Result<Option<TokenInfo>, OAuthError> {
        todo!()
    }

    fn find_token_by_code(&self, code: Code) -> std::result::Result<Option<TokenInfo>, OAuthError> {
        todo!()
    }
}

pub mod helpers;

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AccountManager {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match req.rocket().state::<SharedAccountManager>() {
            None => Outcome::Error((Status::InternalServerError, ())),
            Some(shared_account_manager) => {
                let db = req.guard::<DbConn>().await.unwrap();
                let account_manager_creator = shared_account_manager.account_manager.read().await;
                let account_manager = account_manager_creator(Arc::new(db));
                Outcome::Success(account_manager)
            }
        }
    }
}

pub struct SharedAccountManager {
    pub account_manager: RwLock<AccountManagerCreator>,
}
