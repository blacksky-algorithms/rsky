use crate::account_manager::helpers::auth::{create_service_jwt, ServiceJwtParams};
use anyhow::{anyhow, bail, Result};
use atrium_api::xrpc::http::HeaderMap;
use base64ct::{Base64, Encoding};
use reqwest::header::{HeaderValue, AUTHORIZATION};
use rsky_crypto::types::VerifyOptions;
use rsky_crypto::verify::verify_signature;
use std::time::{Duration, SystemTime};

pub struct ServiceJwtPayload {
    pub iss: String,
    pub aud: String,
    pub exp: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct JwtPayload {
    pub iss: String,
    pub aud: String,
    pub exp: u64,
}

pub async fn create_service_auth_headers(params: ServiceJwtParams) -> Result<HeaderMap> {
    let jwt = create_service_jwt(params).await?;
    let jwt_str = format!("Bearer {jwt}");
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_str(&jwt_str)?);
    Ok(headers)
}

pub fn parse_b64_url_to_json(b64: &str) -> Result<JwtPayload> {
    Ok(serde_json::from_slice::<JwtPayload>(
        Base64::decode_vec(b64)
            .map_err(|err| anyhow!(err.to_string()))?
            .as_slice(),
    )?)
}

pub fn parse_payload(b64: &str) -> Result<JwtPayload> {
    let payload = parse_b64_url_to_json(b64)?;
    Ok(payload)
}

pub async fn verify_jwt<G>(
    jwt_str: String,
    own_did: Option<String>, // None indicates to skip the audience check
    get_signing_key: G,
) -> Result<ServiceJwtPayload>
where
    G: Fn(String, bool) -> Result<String>,
{
    let parts = jwt_str.split(".").collect::<Vec<&str>>();
    match (parts.get(0), parts.get(1), parts.get(2)) {
        (Some(_), Some(parts_1), Some(sig)) if parts.len() == 3 => {
            let parts_1 = *parts_1;
            let sig = *sig;
            let payload = parse_payload(parts_1)?;
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("timestamp in micros since UNIX epoch")
                .as_micros();
            if now > payload.exp as u128 {
                bail!("JwtExpired: jwt expired")
            }
            if own_did.is_some() && payload.aud != own_did.unwrap() {
                bail!("BadJwtAudience: jwt audience does not match service did")
            }
            let msg_bytes = parts[0..2].join(".").into_bytes();
            let sig_bytes = Base64::encode_string(sig.as_bytes())
                .replace("=", "")
                .into_bytes();
            let verify_signature_with_key = |key: String| -> Result<bool> {
                verify_signature(
                    &key,
                    msg_bytes.as_slice(),
                    sig_bytes.as_slice(),
                    Some(VerifyOptions {
                        allow_malleable_sig: Some(true),
                    }),
                )
            };

            let signing_key = get_signing_key(payload.iss.clone(), false)?;

            let mut valid_sig: bool = match verify_signature_with_key(signing_key.clone()) {
                Ok(is_valid) => is_valid,
                Err(err) => {
                    eprintln!("Error received: {}", err);
                    bail!("BadJwtSignature: could not verify jwt signature")
                }
            };

            if !valid_sig {
                // get fresh signing key in case it failed due to a recent rotation
                let fresh_signing_key = get_signing_key(payload.iss.clone(), true)?;
                valid_sig = if fresh_signing_key != signing_key {
                    match verify_signature_with_key(fresh_signing_key) {
                        Ok(is_valid) => is_valid,
                        Err(err) => {
                            eprintln!("Error received: {}", err);
                            bail!("BadJwtSignature: could not verify jwt signature")
                        }
                    }
                } else {
                    false
                };
            }

            if !valid_sig {
                bail!("BadJwtSignature: jwt signature does not match jwt issuer")
            }

            Ok(ServiceJwtPayload {
                iss: payload.iss,
                aud: payload.aud,
                exp: Some(Duration::from_micros(payload.exp)),
            })
        }
        _ => bail!("BadJwt: poorly formatted jwt"),
    }
}
