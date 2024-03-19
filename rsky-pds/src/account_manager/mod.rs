use std::env;
use std::time::SystemTime;
use chrono::offset::Utc as UtcOffset;
use chrono::DateTime;
use helpers::{pcrypt, auth};
use libipld::Cid;
use secp256k1::{Keypair, Secp256k1, SecretKey};
use crate::auth_verifier::AuthScope;

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
    pub fn create_account(opts: CreateAccountOpts) -> Result<()> {
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
            None => None
        };
        // Should be a global var so this only happens once
        let secp = Secp256k1::new();
        let private_key = env::var("PDS_JWT_KEY_K256_PRIVATE_KEY_HEX").unwrap();
        let secret_key =
            SecretKey::from_slice(&hex::decode(private_key.as_bytes()).unwrap()).unwrap();
        let jwt_key = Keypair::from_secret_key(&secp, &secret_key);
        let (access_jwt, refresh_jwt) = auth::create_tokens(auth::CreateTokensOpts{
            did,
            jwt_key,
            service_did: env::var("SERVICE_DID").unwrap(),
            scope: Some(AuthScope::Access),
            jti: None,
            expires_in: None
        })?;
        let refresh_payload = auth::decode_refresh_token(refresh_jwt, jwt_key)?;
        let system_time = SystemTime::now();
        let dt: DateTime<UtcOffset> = system_time.into();
        let now = format!("{}", dt.format("%+"));
        
        if let Some(invite_cod) = invite_code {
            todo!()
        }
        
        Ok(())
    }
}

pub mod helpers;
