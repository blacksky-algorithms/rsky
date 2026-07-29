//! atproto inter-service auth JWTs (spec: XRPC inter-service authentication).
//!
//! Minted by this host when calling a managing app (`checkUserAccess`) and when
//! forwarding notifications; verified when a member's repo host calls
//! `notifyWrite` on this host.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::Digest;

use crate::error::{HostError, Result};
use crate::signing::Signer;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceJwtHeader {
    pub typ: String,
    pub alg: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceClaims {
    pub iss: String,
    pub aud: String,
    pub exp: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lxm: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<u64>,
}

pub const SERVICE_JWT_TTL_SECS: u64 = 60;

/// Mint a service-auth JWT signed by `signer` (sha256 then ECDSA, compact r||s).
pub fn mint(
    signer: &Signer,
    iss: &str,
    aud: &str,
    lxm: &str,
    now: u64,
    jti: String,
) -> Result<String> {
    let header = ServiceJwtHeader {
        typ: "JWT".to_string(),
        alg: rsky_crypto::constants::SECP256K1_JWT_ALG.to_string(),
    };
    let claims = ServiceClaims {
        iss: iss.to_string(),
        aud: aud.to_string(),
        exp: now + SERVICE_JWT_TTL_SECS,
        lxm: Some(lxm.to_string()),
        jti: Some(jti),
        iat: Some(now),
    };
    // Header and claims are plain strings/ints; serialization cannot fail.
    let h = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).expect("header serializes"));
    let c = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).expect("claims serialize"));
    let input = format!("{h}.{c}");
    let sig = signer
        .sign(input.as_bytes())
        .map_err(HostError::Delegation)?;
    Ok(format!("{input}.{}", URL_SAFE_NO_PAD.encode(sig)))
}

/// Decode a service JWT's claims without verifying it (to learn `iss` before
/// resolving the issuer's key). Never trust the result until [`verify`] passes.
pub fn claims(jwt: &str) -> Result<ServiceClaims> {
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() != 3 {
        return Err(HostError::Delegation("malformed service jwt".into()));
    }
    serde_json::from_slice(&b64(parts[1])?)
        .map_err(|e| HostError::Delegation(format!("bad service jwt payload: {e}")))
}

/// Verify a service-auth JWT: audience, expiry, method binding, and signature
/// against the issuer's resolved `did:key`.
pub fn verify(
    jwt: &str,
    accepted_auds: &[&str],
    expected_lxm: &str,
    issuer_did_key: &str,
    now: u64,
) -> Result<ServiceClaims> {
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() != 3 {
        return Err(HostError::Delegation("malformed service jwt".into()));
    }
    let claims: ServiceClaims = serde_json::from_slice(&b64(parts[1])?)
        .map_err(|e| HostError::Delegation(format!("bad service jwt payload: {e}")))?;
    if !accepted_auds.contains(&claims.aud.as_str()) {
        return Err(HostError::Delegation("bad service jwt audience".into()));
    }
    if now >= claims.exp {
        return Err(HostError::Delegation("service jwt expired".into()));
    }
    // lxm is optional in the wild; when present it must bind to this method.
    if let Some(lxm) = &claims.lxm {
        if lxm != expected_lxm {
            return Err(HostError::Delegation("service jwt lxm mismatch".into()));
        }
    }
    let signing_input = format!("{}.{}", parts[0], parts[1]);
    let digest = sha2::Sha256::digest(signing_input.as_bytes());
    let sig = b64(parts[2])?;
    let ok = rsky_crypto::verify::verify_signature_digest(
        &issuer_did_key.to_string(),
        &digest,
        &sig,
        None,
    )
    .map_err(|e| HostError::Delegation(e.to_string()))?;
    if !ok {
        return Err(HostError::Delegation("bad service jwt signature".into()));
    }
    Ok(claims)
}

fn b64(s: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(s)
        .map_err(|e| HostError::Delegation(format!("bad base64: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signing::test_signer;

    const LXM: &str = "com.atproto.space.notifyWrite";

    fn minted() -> (String, Signer) {
        let signer = test_signer();
        let jwt = mint(
            &signer,
            "did:plc:member",
            "did:plc:authority",
            LXM,
            1000,
            "jti-1".to_string(),
        )
        .unwrap();
        (jwt, signer)
    }

    #[test]
    fn mint_verify_roundtrip() {
        let (jwt, signer) = minted();
        let claims = verify(&jwt, &["did:plc:authority"], LXM, signer.did_key(), 1030).unwrap();
        assert_eq!(claims.iss, "did:plc:member");
        assert_eq!(claims.jti.as_deref(), Some("jti-1"));
        assert_eq!(claims.iat, Some(1000));
    }

    #[test]
    fn rejects_wrong_audience_expiry_lxm() {
        let (jwt, signer) = minted();
        assert!(verify(&jwt, &["did:plc:other"], LXM, signer.did_key(), 1030).is_err());
        assert!(verify(&jwt, &["did:plc:authority"], LXM, signer.did_key(), 1060).is_err());
        assert!(verify(
            &jwt,
            &["did:plc:authority"],
            "com.atproto.space.other",
            signer.did_key(),
            1030
        )
        .is_err());
    }

    #[test]
    fn rejects_bad_signature_and_wrong_key() {
        let (jwt, signer) = minted();
        let other = Signer::from_secret(secp256k1::SecretKey::from_slice(&[0x22u8; 32]).unwrap());
        assert!(matches!(
            verify(&jwt, &["did:plc:authority"], LXM, other.did_key(), 1030),
            Err(HostError::Delegation(_))
        ));
        let mut tampered = jwt.clone();
        tampered.push('A');
        assert!(verify(
            &tampered,
            &["did:plc:authority"],
            LXM,
            signer.did_key(),
            1030
        )
        .is_err());
        // Unresolvable key errors surface rather than passing.
        assert!(verify(&jwt, &["did:plc:authority"], LXM, "did:key:zBAD", 1030).is_err());
    }

    #[test]
    fn rejects_malformed_jwts() {
        let (jwt, signer) = minted();
        assert!(verify("a.b", &["x"], LXM, signer.did_key(), 0).is_err());
        assert!(verify("!!.!!.!!", &["x"], LXM, signer.did_key(), 0).is_err());
        let parts: Vec<&str> = jwt.split('.').collect();
        let bad_payload = format!(
            "{}.{}.{}",
            parts[0],
            URL_SAFE_NO_PAD.encode(b"{\"iss\":1}"),
            parts[2]
        );
        assert!(verify(&bad_payload, &["x"], LXM, signer.did_key(), 0).is_err());
    }

    #[test]
    fn claims_peek_decodes_without_verifying() {
        let (jwt, _) = minted();
        let peeked = claims(&jwt).unwrap();
        assert_eq!(peeked.iss, "did:plc:member");
        assert!(claims("x.y").is_err());
        assert!(claims("x.!!.z").is_err());
        // Valid base64 that is not a claims object.
        assert!(claims("e30.e30.e30").is_err());
    }

    #[test]
    fn missing_lxm_is_accepted() {
        // Some issuers omit lxm; the binding is only enforced when present.
        let signer = test_signer();
        let header = ServiceJwtHeader {
            typ: "JWT".to_string(),
            alg: "ES256K".to_string(),
        };
        let claims = ServiceClaims {
            iss: "did:plc:member".to_string(),
            aud: "did:plc:authority".to_string(),
            exp: 2000,
            lxm: None,
            jti: None,
            iat: None,
        };
        let h = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
        let c = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
        let input = format!("{h}.{c}");
        let sig = signer.sign(input.as_bytes()).unwrap();
        let jwt = format!("{input}.{}", URL_SAFE_NO_PAD.encode(sig));
        let got = verify(&jwt, &["did:plc:authority"], LXM, signer.did_key(), 1000).unwrap();
        assert!(got.lxm.is_none());
    }
}
