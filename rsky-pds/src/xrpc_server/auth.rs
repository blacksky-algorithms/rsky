use anyhow::{anyhow, bail, Result};
use base64ct::{Base64, Encoding};
use chrono::Duration;
use serde_json::Value;
use std::time::SystemTime;

pub struct ServiceJwtPayload {
    pub iss: String,
    pub aud: String,
    pub exp: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct JwtPayload {
    pub iss: String,
    pub aud: String,
    pub exp: u128,
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
        (Some(parts_0), Some(parts_1), Some(sig)) if parts.len() == 3 => {
            let parts_1 = *parts_1;
            let sig = *sig;
            let payload = parse_payload(parts_1)?;
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("timestamp in micros since UNIX epoch")
                .as_micros();
            if now > payload.exp {
                bail!("JwtExpired: jwt expired")
            }
            if own_did.is_some() && payload.aud != own_did.unwrap() {
                bail!("BadJwtAudience: jwt audience does not match service did")
            }
            let msg_bytes = parts[0..2].join(".").into_bytes();
            let sig_bytes = Base64::encode_string(sig.as_bytes())
                .replace("=", "")
                .into_bytes();

            todo!()
        }
        _ => bail!("BadJwt: poorly formatted jwt"),
    }
}
