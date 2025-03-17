use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::AccessStandard;
use crate::config::ServerConfig;
use rocket::serde::json::Json;
use rocket::State;
use rsky_crypto::utils::encode_did_key;
use rsky_lexicon::com::atproto::identity::GetRecommendedDidCredentialsResponse;
use secp256k1::{Keypair, Secp256k1, SecretKey};
use serde_json::json;
use std::env;

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.identity.getRecommendedDidCredentials")]
pub async fn get_recommended_did_credentials(
    auth: AccessStandard,
    cfg: &State<ServerConfig>,
) -> Result<Json<GetRecommendedDidCredentialsResponse>, ApiError> {
    let requester = auth.access.credentials.unwrap().did.unwrap();
    let availability_flags = AvailabilityFlags {
        include_taken_down: Some(true),
        include_deactivated: Some(true),
    };
    let account = AccountManager::get_account_legacy(&requester, Some(availability_flags))
        .await?
        .expect("Account not found despite valid access");

    let mut also_known_as = Vec::new();
    match account.handle {
        None => {}
        Some(res) => {
            also_known_as.push("at://".to_string() + res.as_str());
        }
    }

    let signing_key = get_public_signing_key()?;
    let verification_methods = json!({
        "atproto": signing_key
    });

    let rotation_key = get_public_rotation_key()?;
    let rotation_keys = vec![rotation_key];

    let services = json!({
        "atproto_pds": {
            "type": "AtprotoPersonalDataServer",
            "endpoint": cfg.service.public_url
        }
    });
    let response = GetRecommendedDidCredentialsResponse {
        also_known_as,
        verification_methods,
        rotation_keys,
        services,
    };
    Ok(Json(response))
}

fn get_public_rotation_key() -> Result<String, ApiError> {
    let secp = Secp256k1::new();
    let private_rotation_key = match env::var("PDS_PLC_ROTATION_KEY_K256_PRIVATE_KEY_HEX") {
        Ok(res) => res,
        Err(error) => {
            tracing::error!("Error geting rotation private key\n{error}");
            return Err(ApiError::RuntimeError);
        }
    };
    match hex::decode(private_rotation_key.as_bytes()) {
        Ok(bytes) => match SecretKey::from_slice(&bytes) {
            Ok(secret_key) => {
                let rotation_keypair = Keypair::from_secret_key(&secp, &secret_key);
                Ok(encode_did_key(&rotation_keypair.public_key()))
            }
            Err(error) => {
                tracing::error!("Error geting rotation secret key from bytes\n{error}");
                Err(ApiError::RuntimeError)
            }
        },
        Err(error) => {
            tracing::error!("Unable to hex decode rotation key\n{error}");
            Err(ApiError::RuntimeError)
        }
    }
}

fn get_public_signing_key() -> Result<String, ApiError> {
    let secp = Secp256k1::new();
    let private_signing_key = match env::var("PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX") {
        Ok(res) => res,
        Err(error) => {
            tracing::error!("Error geting signing private key\n{error}");
            return Err(ApiError::RuntimeError);
        }
    };
    match hex::decode(private_signing_key.as_bytes()) {
        Ok(bytes) => match SecretKey::from_slice(&bytes) {
            Ok(secret_key) => {
                let signing_keypair = Keypair::from_secret_key(&secp, &secret_key);
                Ok(encode_did_key(&signing_keypair.public_key()))
            }
            Err(error) => {
                tracing::error!("Error geting signing secret key from bytes\n{error}");
                Err(ApiError::RuntimeError)
            }
        },
        Err(error) => {
            tracing::error!("Unable to hex decode signing key\n{error}");
            Err(ApiError::RuntimeError)
        }
    }
}
