use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::AccessStandard;
use rocket::serde::json::Json;
use rsky_crypto::utils::encode_did_key;
use rsky_lexicon::com::atproto::identity::GetRecommendedDidCredentialsResponse;
use secp256k1::{Keypair, Secp256k1, SecretKey};
use serde_json::json;
use std::env;

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.identity.getRecommendedDidCredentials")]
pub async fn get_recommended_did_credentials(
    auth: AccessStandard,
) -> Result<Json<GetRecommendedDidCredentialsResponse>, ApiError> {
    let requester = auth.access.credentials.unwrap().did.unwrap();
    let availability_flags = AvailabilityFlags {
        include_taken_down: Some(true),
        include_deactivated: Some(true),
    };
    let account = AccountManager::get_account(&requester, Some(availability_flags))
        .await?
        .expect("Account not found despite valid access");

    let mut also_known_as = Vec::new();
    match account.handle {
        None => {}
        Some(res) => {
            also_known_as.push("at://".to_string() + res.as_str());
        }
    }

    let secp = Secp256k1::new();
    let signing_private_key =
        env::var("PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX").expect("Signing Key Missing");
    let signing_secret_key =
        SecretKey::from_slice(&hex::decode(signing_private_key.as_bytes()).unwrap()).unwrap();
    let signing_keypair = Keypair::from_secret_key(&secp, &signing_secret_key);
    let verification_methods = json!({
        "atproto": encode_did_key(&signing_keypair.public_key())
    });

    let mut rotation_keys = Vec::new();
    let secp = Secp256k1::new();
    let private_rotation_key =
        env::var("PDS_PLC_ROTATION_KEY_K256_PRIVATE_KEY_HEX").expect("Rotation Key Missing");
    let private_secret_key =
        SecretKey::from_slice(&hex::decode(private_rotation_key.as_bytes()).unwrap()).unwrap();
    let rotation_keypair = Keypair::from_secret_key(&secp, &private_secret_key);
    rotation_keys.push(encode_did_key(&rotation_keypair.public_key()));

    let endpoint = format!("https://{}", env::var("PDS_HOSTNAME").unwrap());
    let services = json!({
        "atproto_pds": {
            "type": "AtprotoPersonalDataServer".to_string(),
            "endpoint": endpoint
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
