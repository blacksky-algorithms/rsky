use crate::account_manager::helpers::repo;
use crate::auth_verifier::AuthScope;
use anyhow::Result;
use chrono::offset::Utc as UtcOffset;
use chrono::DateTime;
use helpers::{account, auth, invite, pcrypt};
use libipld::Cid;
use secp256k1::{Keypair, Secp256k1, SecretKey};
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

pub struct AccountManager {}

impl AccountManager {
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
            Some(password) => Some(pcrypt::gen_salt_and_hash(password)?),
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
        let system_time = SystemTime::now();
        let dt: DateTime<UtcOffset> = system_time.into();
        let now = format!("{}", dt.format("%+"));

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
}

pub mod helpers;
