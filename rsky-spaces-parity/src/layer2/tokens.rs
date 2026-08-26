//! The credentials the two servers demand, minted locally.
//!
//! The space host verifies an access token it did not issue, so the gate acts as
//! the authorization server: it holds the shared HS256 secret the host is
//! configured with and signs `at+jwt` tokens with it. DPoP proofs are ES256 over
//! a fixed P-256 key so a proof can be rebuilt for every request.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use rsky_oauth::jwk::{EcCurve, Jwk};
use rsky_oauth::jwt::{sign, JwtClaims, JwtHeader};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};

pub const DPOP_KEY_BYTES: [u8; 32] = [0x42u8; 32];
pub const CLIENT_ID: &str = "https://layer2.invalid/oauth-client-metadata.json";

static JTI: AtomicU64 = AtomicU64::new(0);

fn next_jti(prefix: &str) -> String {
    format!("{prefix}-{}", JTI.fetch_add(1, Ordering::SeqCst))
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .expect("clock before unix epoch")
        .as_secs()
}

pub fn dpop_key() -> Jwk {
    Jwk::from_private_key_bytes(EcCurve::P256, &DPOP_KEY_BYTES).expect("dpop key")
}

pub fn jwk_thumbprint_of_dpop_key() -> String {
    let public = serde_json::to_value(dpop_key().to_public()).expect("jwk serializes");
    let canonical = format!(
        r#"{{"crv":"{}","kty":"{}","x":"{}","y":"{}"}}"#,
        public["crv"].as_str().unwrap_or_default(),
        public["kty"].as_str().unwrap_or_default(),
        public["x"].as_str().unwrap_or_default(),
        public["y"].as_str().unwrap_or_default(),
    );
    URL_SAFE_NO_PAD.encode(Sha256::digest(canonical.as_bytes()))
}

/// A DPoP proof bound to `method` and `url`. `url` carries no query string:
/// both servers compare only scheme, host and path.
pub fn dpop_proof(method: &str, url: &str, access_token: Option<&str>) -> String {
    let key = dpop_key();
    let mut header = JwtHeader::new("ES256");
    header.typ = Some("dpop+jwt".to_string());
    header.jwk = Some(key.to_public());
    let mut claims = JwtClaims {
        iat: Some(now()),
        jti: Some(next_jti("proof")),
        ..Default::default()
    };
    claims.extra.insert("htm".to_string(), method.into());
    claims.extra.insert("htu".to_string(), url.into());
    if let Some(token) = access_token {
        claims.extra.insert(
            "ath".to_string(),
            URL_SAFE_NO_PAD
                .encode(Sha256::digest(token.as_bytes()))
                .into(),
        );
    }
    sign(&header, &claims, &key).expect("dpop proof signs")
}

/// An HS256 `at+jwt` access token of the shape a PDS that is its own
/// authorization server issues: DID audience, no `scope`, DPoP-bound.
pub fn access_token(secret: &str, issuer: &str, audience: &str, subject: &str) -> String {
    let header = json!({"typ": "at+jwt", "alg": "HS256"});
    let claims = json!({
        "iss": issuer,
        "aud": audience,
        "sub": subject,
        "iat": now(),
        "exp": now() + 3600,
        "jti": next_jti("tok"),
        "client_id": CLIENT_ID,
        "cnf": { "jkt": jwk_thumbprint_of_dpop_key() },
    });
    let input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).expect("header serializes")),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).expect("claims serialize")),
    );
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(input.as_bytes());
    format!(
        "{input}.{}",
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    )
}

/// An inter-service auth JWT, minted with the space host's own issuer code so
/// the gate cannot drift from what the host verifies.
pub fn service_jwt(
    signer: &rsky_space_host::signing::Signer,
    iss: &str,
    aud: &str,
    lxm: &str,
) -> anyhow::Result<String> {
    rsky_space_host::service_jwt::mint(signer, iss, aud, lxm, now(), next_jti("svc"))
        .map_err(|error| anyhow::anyhow!("service jwt: {error}"))
}
