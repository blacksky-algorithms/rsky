use crate::account_manager::helpers::account::{
    AccountStatus, ActorAccount, AvailabilityFlags, GetAccountAdminStatusOutput,
};
use crate::account_manager::helpers::auth::{
    AuthHelperError, CreateTokensOpts, RefreshGracePeriodOpts,
};
use crate::account_manager::helpers::invite::{CodeDetail, CodeUse};
use crate::account_manager::helpers::password::UpdateUserPasswordOpts;
use crate::account_manager::helpers::repo;
use crate::auth_verifier::AuthScope;
use crate::common;
use crate::common::time::{from_micros_to_str, from_str_to_micros, HOUR};
use crate::common::RFC3339_VARIANT;
use crate::models::models::EmailTokenPurpose;
use anyhow::Result;
use chrono::offset::Utc as UtcOffset;
use chrono::DateTime;
use futures::try_join;
use helpers::{account, auth, email_token, invite, password};
use libipld::Cid;
use rsky_lexicon::com::atproto::admin::StatusAttr;
use rsky_lexicon::com::atproto::server::{AccountCodes, CreateAppPasswordOutput};
use secp256k1::{Keypair, Secp256k1, SecretKey};
use std::collections::BTreeMap;
use std::env;
use std::time::SystemTime;

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

#[derive(Clone)]
pub struct AccountManager {}

impl AccountManager {
    pub async fn get_account(
        handle_or_did: &String,
        flags: Option<AvailabilityFlags>,
    ) -> Result<Option<ActorAccount>> {
        account::get_account(handle_or_did, flags).await
    }

    pub async fn get_account_by_email(
        email: &String,
        flags: Option<AvailabilityFlags>,
    ) -> Result<Option<ActorAccount>> {
        account::get_account_by_email(email, flags).await
    }

    pub async fn is_account_activated(did: &String) -> Result<bool> {
        let account = Self::get_account(
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
        handle_or_did: &String,
        flags: Option<AvailabilityFlags>,
    ) -> Result<Option<String>> {
        match Self::get_account(handle_or_did, flags).await {
            Ok(Some(got)) => Ok(Some(got.did)),
            _ => Ok(None),
        }
    }

    pub async fn create_account(opts: CreateAccountOpts) -> Result<(String, String)> {
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
        let private_key = env::var("PDS_JWT_KEY_K256_PRIVATE_KEY_HEX").unwrap();
        let secret_key =
            SecretKey::from_slice(&hex::decode(private_key.as_bytes()).unwrap()).unwrap();
        let jwt_key = Keypair::from_secret_key(&secp, &secret_key);
        let (access_jwt, refresh_jwt) = auth::create_tokens(auth::CreateTokensOpts {
            did: did.clone(),
            jwt_key,
            service_did: env::var("PDS_SERVICE_DID").unwrap(),
            scope: Some(AuthScope::Access),
            jti: None,
            expires_in: None,
        })?;
        let refresh_payload = auth::decode_refresh_token(refresh_jwt.clone(), jwt_key)?;
        let now = common::now();

        if let Some(invite_code) = invite_code.clone() {
            invite::ensure_invite_is_available(invite_code).await?;
        }
        account::register_actor(did.clone(), handle, deactivated)?;
        if let (Some(email), Some(password_encrypted)) = (email, password_encrypted) {
            account::register_account(did.clone(), email, password_encrypted)?;
        }
        invite::record_invite_use(did.clone(), invite_code, now)?;
        auth::store_refresh_token(refresh_payload, None).await?;
        repo::update_root(did, repo_cid, repo_rev)?;
        Ok((access_jwt, refresh_jwt))
    }

    pub async fn get_account_admin_status(
        did: &String,
    ) -> Result<Option<GetAccountAdminStatusOutput>> {
        account::get_account_admin_status(did).await
    }

    pub fn update_repo_root(did: String, cid: Cid, rev: String) -> Result<()> {
        Ok(repo::update_root(did, cid, rev)?)
    }

    pub async fn delete_account(did: &String) -> Result<()> {
        account::delete_account(did).await
    }

    pub async fn takedown_account(did: &String, takedown: StatusAttr) -> Result<()> {
        (_, _) = try_join!(
            account::update_account_takedown_status(did, takedown),
            auth::revoke_refresh_tokens_by_did(did)
        )?;
        Ok(())
    }

    // @NOTE should always be paired with a sequenceHandle().
    pub async fn update_handle(did: &String, handle: &String) -> Result<()> {
        account::update_handle(did, handle).await
    }

    pub async fn deactivate_account(did: &String, delete_after: Option<String>) -> Result<()> {
        account::deactivate_account(did, delete_after).await
    }

    pub async fn activate_account(did: &String) -> Result<()> {
        account::activate_account(did).await
    }

    pub async fn get_account_status(handle_or_did: &String) -> Result<AccountStatus> {
        let got = account::get_account(
            handle_or_did,
            Some(AvailabilityFlags {
                include_deactivated: Some(true),
                include_taken_down: Some(true),
            }),
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
        did: String,
        app_password_name: Option<String>,
    ) -> Result<(String, String)> {
        let secp = Secp256k1::new();
        let private_key = env::var("PDS_JWT_KEY_K256_PRIVATE_KEY_HEX").unwrap();
        let secret_key =
            SecretKey::from_slice(&hex::decode(private_key.as_bytes()).unwrap()).unwrap();
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
        auth::store_refresh_token(refresh_payload, app_password_name).await?;
        Ok((access_jwt, refresh_jwt))
    }

    pub async fn rotate_refresh_token(id: &String) -> Result<Option<(String, String)>> {
        let token = auth::get_refresh_token(id).await?;
        if let Some(token) = token {
            let system_time = SystemTime::now();
            let dt: DateTime<UtcOffset> = system_time.into();
            let now = format!("{}", dt.format(RFC3339_VARIANT));

            // take the chance to tidy all of a user's expired tokens
            // does not need to be transactional since this is just best-effort
            auth::delete_expired_refresh_tokens(&token.did, now).await?;

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
            let next_id = token
                .next_id
                .unwrap_or_else(|| auth::get_refresh_token_id());

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
                auth::add_refresh_grace_period(RefreshGracePeriodOpts {
                    id: id.clone(),
                    expires_at: from_micros_to_str(expires_at),
                    next_id
                }),
                auth::store_refresh_token(refresh_payload, token.app_password_name)
            ) {
                Ok(_) => Ok(Some((access_jwt, refresh_jwt))),
                Err(e) => match e.downcast_ref() {
                    Some(AuthHelperError::ConcurrentRefresh) => {
                        Box::pin(Self::rotate_refresh_token(id)).await
                    }
                    _ => Err(e),
                },
            }
        } else {
            Ok(None)
        }
    }

    pub async fn revoke_refresh_token(id: String) -> Result<bool> {
        auth::revoke_refresh_token(id).await
    }
    // Invites
    // ----------

    pub async fn ensure_invite_is_available(code: String) -> Result<()> {
        invite::ensure_invite_is_available(code).await
    }

    pub async fn create_invite_codes(to_create: Vec<AccountCodes>, use_count: i32) -> Result<()> {
        invite::create_invite_codes(to_create, use_count).await
    }

    pub async fn create_account_invite_codes(
        for_account: &String,
        codes: Vec<String>,
        expected_total: usize,
        disabled: bool,
    ) -> Result<Vec<CodeDetail>> {
        invite::create_account_invite_codes(for_account, codes, expected_total, disabled).await
    }

    pub async fn get_account_invite_codes(did: &String) -> Result<Vec<CodeDetail>> {
        invite::get_account_invite_codes(did).await
    }

    pub async fn get_invited_by_for_accounts(
        dids: Vec<&String>,
    ) -> Result<BTreeMap<String, CodeDetail>> {
        invite::get_invited_by_for_accounts(dids).await
    }

    pub async fn get_invite_codes_uses(
        codes: Vec<String>,
    ) -> Result<BTreeMap<String, Vec<CodeUse>>> {
        invite::get_invite_codes_uses(codes).await
    }

    pub async fn set_account_invites_disabled(did: &String, disabled: bool) -> Result<()> {
        invite::set_account_invites_disabled(did, disabled).await
    }

    pub async fn disable_invite_codes(opts: DisableInviteCodesOpts) -> Result<()> {
        invite::disable_invite_codes(opts).await
    }

    // Passwords
    // ----------

    pub async fn create_app_password(did: String, name: String) -> Result<CreateAppPasswordOutput> {
        password::create_app_password(did, name).await
    }

    pub async fn list_app_passwords(did: &String) -> Result<Vec<(String, String)>> {
        password::list_app_passwords(did).await
    }

    pub async fn verify_account_password(did: &String, password_str: &String) -> Result<bool> {
        password::verify_account_password(did, password_str).await
    }

    pub async fn verify_app_password(
        did: &String,
        password_str: &String,
    ) -> Result<Option<String>> {
        password::verify_app_password(did, password_str).await
    }

    pub async fn reset_password(opts: ResetPasswordOpts) -> Result<()> {
        let did = email_token::assert_valid_token_and_find_did(
            EmailTokenPurpose::ResetPassword,
            &opts.token,
            None,
        )
        .await?;
        Self::update_account_password(UpdateAccountPasswordOpts {
            did,
            password: opts.password,
        })
        .await
    }

    pub async fn update_account_password(opts: UpdateAccountPasswordOpts) -> Result<()> {
        let UpdateAccountPasswordOpts { did, .. } = opts;
        let password_encrypted = password::gen_salt_and_hash(opts.password)?;
        try_join!(
            password::update_user_password(UpdateUserPasswordOpts {
                did: did.clone(),
                password_encrypted
            }),
            email_token::delete_email_token(&did, EmailTokenPurpose::ResetPassword),
            auth::revoke_refresh_tokens_by_did(&did)
        )?;
        Ok(())
    }

    pub async fn revoke_app_password(did: String, name: String) -> Result<()> {
        try_join!(
            password::delete_app_password(&did, &name),
            auth::revoke_app_password_refresh_token(&did, &name)
        )?;
        Ok(())
    }

    // Email Tokens
    // ----------
    pub async fn confirm_email<'em>(opts: ConfirmEmailOpts<'em>) -> Result<()> {
        let ConfirmEmailOpts { did, token } = opts;
        email_token::assert_valid_token(did, EmailTokenPurpose::ConfirmEmail, token, None).await?;
        let now = common::now();
        try_join!(
            email_token::delete_email_token(did, EmailTokenPurpose::ConfirmEmail),
            account::set_email_confirmed_at(did, now)
        )?;
        Ok(())
    }

    pub async fn update_email(opts: UpdateEmailOpts) -> Result<()> {
        let UpdateEmailOpts { did, email } = opts;
        try_join!(
            account::update_email(&did, &email),
            email_token::delete_all_email_tokens(&did)
        )?;
        Ok(())
    }

    pub async fn assert_valid_email_token(
        did: &String,
        purpose: EmailTokenPurpose,
        token: &String,
    ) -> Result<()> {
        email_token::assert_valid_token(did, purpose, token, None).await
    }

    pub async fn create_email_token(did: &String, purpose: EmailTokenPurpose) -> Result<String> {
        email_token::create_email_token(did, purpose).await
    }
}

pub mod helpers;
