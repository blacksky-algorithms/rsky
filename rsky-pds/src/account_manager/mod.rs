use crate::account_manager::helpers::account::{ActorAccount, AvailabilityFlags};
use crate::account_manager::helpers::invite::CodeDetail;
use crate::account_manager::helpers::repo;
use crate::auth_verifier::AuthScope;
use crate::models::models::EmailTokenPurpose;
use anyhow::Result;
use helpers::{account, auth, email_token, invite, password};
use libipld::Cid;
use rsky_lexicon::com::atproto::server::{AccountCodes, CreateAppPasswordOutput};
use secp256k1::{Keypair, Secp256k1, SecretKey};
use std::env;
use futures::try_join;
use crate::common;

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
    pub token: &'em String
}

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

    pub fn create_account(opts: CreateAccountOpts) -> Result<(String, String)> {
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
            invite::ensure_invite_is_available(invite_code)?;
        }
        account::register_actor(did.clone(), handle, deactivated)?;
        if let (Some(email), Some(password_encrypted)) = (email, password_encrypted) {
            account::register_account(did.clone(), email, password_encrypted)?;
        }
        invite::record_invite_use(did.clone(), invite_code, now)?;
        auth::store_refresh_token(refresh_payload, None)?;
        repo::update_root(did, repo_cid, repo_rev)?;
        Ok((access_jwt, refresh_jwt))
    }

    pub fn update_repo_root(did: String, cid: Cid, rev: String) -> Result<()> {
        Ok(repo::update_root(did, cid, rev)?)
    }

    pub async fn delete_account(did: &String) -> Result<()> {
        account::delete_account(did).await
    }

    pub async fn activate_account(did: &String) -> Result<()> {
        account::activate_account(did).await
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
        let (access_jwt, refresh_jwt) = auth::create_tokens(auth::CreateTokensOpts {
            did,
            jwt_key,
            service_did: env::var("PDS_SERVICE_DID").unwrap(),
            scope: Some(scope),
            jti: None,
            expires_in: None,
        })?;
        let refresh_payload = auth::decode_refresh_token(refresh_jwt.clone(), jwt_key)?;
        auth::store_refresh_token(refresh_payload, app_password_name)?;
        Ok((access_jwt, refresh_jwt))
    }

    pub async fn revoke_refresh_token(id: String) -> Result<bool> {
        auth::revoke_refresh_token(id).await
    }
    // Invites
    // ----------

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

    // Passwords
    // ----------

    pub async fn create_app_password(did: String, name: String) -> Result<CreateAppPasswordOutput> {
        password::create_app_password(did, name).await
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
    
    pub async fn assert_valid_email_token(
        did: &String,
        purpose: EmailTokenPurpose,
        token: &String,
    ) -> Result<()> {
        email_token::assert_valid_token(did, purpose, token, None).await
    }
}

pub mod helpers;
