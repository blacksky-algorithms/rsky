use rsky_crypto::constants::DID_KEY_PREFIX;
use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::AccessStandard;
use crate::db::DbConn;
use secp256k1::{Keypair, PublicKey, Secp256k1, SecretKey};
use std::collections::BTreeMap;
use std::env;
use rocket::serde::json::Json;
use rsky_crypto::utils::encode_did_key;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecommendedService {
    pub r#type: String,
    pub endpoint: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VerificationMethod {
    pub atproto: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GetRecommendedDidCredentialsResponse {
    pub also_known_as: Vec<String>,
    pub verification_methods: VerificationMethod,
    pub rotation_keys: Vec<String>,
    pub services: BTreeMap<String, RecommendedService>,
}

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

    //TODO Seperate signing key logic into seperate module
    let secp = Secp256k1::new();
    let signing_private_key =
        env::var("PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX").expect("Signing Key Missing");
    let signing_secret_key =
        SecretKey::from_slice(&hex::decode(signing_private_key.as_bytes()).unwrap()).unwrap();
    let signing_keypair = Keypair::from_secret_key(&secp, &signing_secret_key);
    let verification_methods = VerificationMethod {
        atproto: encode_did_key(&signing_keypair.public_key()),
    };

    //TODO seperate rotation key logic into separate module
    let mut rotation_keys = Vec::new();
    let secp = Secp256k1::new();
    let private_rotation_key =
        env::var("PDS_PLC_ROTATION_KEY_K256_PRIVATE_KEY_HEX").expect("Rotation Key Missing");
    let private_secret_key =
        SecretKey::from_slice(&hex::decode(private_rotation_key.as_bytes()).unwrap()).unwrap();
    let rotation_keypair = Keypair::from_secret_key(&secp, &private_secret_key);
    rotation_keys.push(encode_did_key(&rotation_keypair.public_key()));

    let mut services = BTreeMap::new();
    //TODO Add handling for if this is down
    let endpoint = format!("https://{}", env::var("PDS_HOSTNAME").unwrap());
    services.insert(
        "atproto_pds".to_string(),
        RecommendedService {
            r#type: "AtprotoPersonalDataServer".to_string(),
            endpoint,
        },
    );
    let response = GetRecommendedDidCredentialsResponse {
        also_known_as,
        verification_methods,
        rotation_keys,
        services,
    };
    Ok(Json(response))
}