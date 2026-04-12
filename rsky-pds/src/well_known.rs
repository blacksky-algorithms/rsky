use crate::account_manager::AccountManager;
use crate::config::ServerConfig;
use anyhow::Result;
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome};
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::{Request, State};
use serde::Serialize;

pub struct HostHeader(pub String);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for HostHeader {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match req.headers().get_one("Host") {
            Some(h) => Outcome::Success(HostHeader(h.to_string())),
            None => Outcome::Forward(Status::InternalServerError),
        }
    }
}

#[rocket::get("/.well-known/atproto-did")]
pub async fn well_known(
    host: HostHeader,
    cfg: &State<ServerConfig>,
    account_manager: AccountManager,
) -> Result<String, status::Custom<String>> {
    let handle = host.0;
    let supported_handle = cfg
        .identity
        .service_handle_domains
        .iter()
        .any(|host| handle.ends_with(host.as_str()) || handle == host[1..]);
    if !supported_handle {
        return Err(status::Custom(
            Status::NotFound,
            "User not found".to_string(),
        ));
    }
    match account_manager.get_account(&handle, None).await {
        Ok(user) => {
            let did: Option<String> = match user {
                Some(user) => Some(user.did),
                None => None,
            };
            match did {
                None => Err(status::Custom(
                    Status::NotFound,
                    "User not found".to_string(),
                )),
                Some(did) => Ok(did),
            }
        }
        Err(_) => Err(status::Custom(
            Status::InternalServerError,
            "Internal Server Error".to_string(),
        )),
    }
}

/// did:web DID document for service-to-service auth.
/// Resolves did:web:{hostname} -> /.well-known/did.json
#[derive(Serialize)]
pub struct DidDocument {
    #[serde(rename = "@context")]
    context: Vec<String>,
    id: String,
    #[serde(rename = "verificationMethod")]
    verification_method: Vec<VerificationMethod>,
    service: Vec<DidService>,
}

#[derive(Serialize)]
struct VerificationMethod {
    id: String,
    #[serde(rename = "type")]
    type_: String,
    controller: String,
    #[serde(rename = "publicKeyMultibase")]
    public_key_multibase: String,
}

#[derive(Serialize)]
struct DidService {
    id: String,
    #[serde(rename = "type")]
    type_: String,
    #[serde(rename = "serviceEndpoint")]
    service_endpoint: String,
}

#[rocket::get("/.well-known/did.json")]
pub async fn did_json(
    cfg: &State<ServerConfig>,
) -> Result<Json<DidDocument>, status::Custom<String>> {
    let hostname = &cfg.service.hostname;
    let did = format!("did:web:{}", hostname);

    // Derive public key multibase from the PDS signing key
    let signing_key_hex = std::env::var("PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX")
        .unwrap_or_default();

    let public_key_multibase = if !signing_key_hex.is_empty() {
        match derive_public_key_multibase(&signing_key_hex) {
            Ok(mb) => mb,
            Err(e) => {
                tracing::error!("Failed to derive public key: {e}");
                return Err(status::Custom(
                    Status::InternalServerError,
                    "Failed to derive signing key".to_string(),
                ));
            }
        }
    } else {
        return Err(status::Custom(
            Status::InternalServerError,
            "PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX not set".to_string(),
        ));
    };

    Ok(Json(DidDocument {
        context: vec![
            "https://www.w3.org/ns/did/v1".to_string(),
            "https://w3id.org/security/multikey/v1".to_string(),
        ],
        id: did.clone(),
        verification_method: vec![VerificationMethod {
            id: format!("{}#atproto", did),
            type_: "Multikey".to_string(),
            controller: did.clone(),
            public_key_multibase,
        }],
        service: vec![DidService {
            id: "#atproto_pds".to_string(),
            type_: "AtprotoPersonalDataServer".to_string(),
            service_endpoint: format!("https://{}", hostname),
        }],
    }))
}

fn derive_public_key_multibase(hex_privkey: &str) -> Result<String, String> {
    use secp256k1::{Secp256k1, SecretKey};

    let privkey_bytes = hex::decode(hex_privkey)
        .map_err(|e| format!("hex decode: {e}"))?;
    let secp = Secp256k1::new();
    let sk = SecretKey::from_slice(&privkey_bytes)
        .map_err(|e| format!("secret key: {e}"))?;
    let pk = sk.public_key(&secp);
    let compressed = pk.serialize(); // 33 bytes compressed

    // Multicodec secp256k1-pub prefix: 0xe7 0x01
    let mut prefixed = vec![0xe7, 0x01];
    prefixed.extend_from_slice(&compressed);

    // Multibase base58btc encoding: 'z' prefix + base58btc
    Ok(multibase::encode(multibase::Base::Base58Btc, prefixed))
}
