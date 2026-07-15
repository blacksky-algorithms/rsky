//! Minimal EC JWK (P-256) support for verifying ES256-signed JWTs
//! (proposal §Client attestation).
//!
//! A space authority verifies a client attestation by resolving the client's
//! published JWKS and checking the JWT signature against the key named by the
//! attestation's `kid`. Only `kty: "EC"` / `crv: "P-256"` keys are supported,
//! matching the attestation's required `alg: "ES256"`.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::error::{Result, SpaceError};

const COORD_LEN: usize = 32;

/// An EC public JWK restricted to the P-256 curve.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EcJwk {
    pub kty: String,
    pub crv: String,
    /// base64url (no padding) x coordinate, 32 bytes decoded.
    pub x: String,
    /// base64url (no padding) y coordinate, 32 bytes decoded.
    pub y: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kid: Option<String>,
}

impl EcJwk {
    /// Parse a JWK from JSON and validate its key type and curve.
    pub fn from_json(json: &str) -> Result<Self> {
        let jwk: EcJwk = serde_json::from_str(json)?;
        jwk.validate()?;
        Ok(jwk)
    }

    pub fn validate(&self) -> Result<()> {
        if self.kty != "EC" {
            return Err(SpaceError::InvalidJwk(format!(
                "unsupported kty: {}",
                self.kty
            )));
        }
        if self.crv != "P-256" {
            return Err(SpaceError::InvalidJwk(format!(
                "unsupported crv: {}",
                self.crv
            )));
        }
        Ok(())
    }

    /// The SEC1 uncompressed point: `0x04 || x || y`.
    pub fn sec1_point(&self) -> Result<Vec<u8>> {
        let x = decode_coord(&self.x, "x")?;
        let y = decode_coord(&self.y, "y")?;
        let mut point = Vec::with_capacity(1 + COORD_LEN * 2);
        point.push(0x04);
        point.extend_from_slice(&x);
        point.extend_from_slice(&y);
        Ok(point)
    }
}

fn decode_coord(b64: &str, name: &str) -> Result<Vec<u8>> {
    let bytes = URL_SAFE_NO_PAD
        .decode(b64)
        .map_err(|e| SpaceError::InvalidJwk(format!("{name} is not base64url: {e}")))?;
    if bytes.len() != COORD_LEN {
        return Err(SpaceError::InvalidJwk(format!(
            "{name} must be {COORD_LEN} bytes, got {}",
            bytes.len()
        )));
    }
    Ok(bytes)
}

/// Verify an ES256 signature over a JWT signing input
/// (`header_b64.payload_b64` bytes) against a P-256 JWK.
///
/// `sig` must be the compact 64-byte low-S `r || s` encoding; anything else is
/// rejected as a bad signature.
pub fn verify_es256(jwk: &EcJwk, signing_input: &[u8], sig: &[u8]) -> Result<()> {
    jwk.validate()?;
    let point = jwk.sec1_point()?;
    let ok = rsky_crypto::p256::operations::verify_sig(&point, signing_input, sig, None)
        .map_err(|e| SpaceError::Crypto(e.to_string()))?;
    if ok {
        Ok(())
    } else {
        Err(SpaceError::BadSignature)
    }
}

/// A JWKS document (`jwks` / `jwks_uri` in client metadata).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JwkSet {
    pub keys: Vec<EcJwk>,
}

impl JwkSet {
    /// Parse a JWKS from JSON without validating individual keys; callers
    /// validate the one key they select (a set may carry non-P-256 keys).
    pub fn from_json(json: &str) -> Result<Self> {
        Ok(serde_json::from_str(json)?)
    }

    /// The key with the given `kid`, if any.
    pub fn find(&self, kid: &str) -> Option<&EcJwk> {
        self.keys.iter().find(|k| k.kid.as_deref() == Some(kid))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::signature::hazmat::PrehashSigner;
    use p256::ecdsa::{Signature, SigningKey};
    use sha2::{Digest, Sha256};

    fn signing_key() -> SigningKey {
        SigningKey::from_slice(&[0x61u8; 32]).unwrap()
    }

    fn jwk_for(key: &SigningKey, kid: Option<&str>) -> EcJwk {
        let point = key.verifying_key().to_encoded_point(false);
        let bytes = point.as_bytes();
        EcJwk {
            kty: "EC".to_string(),
            crv: "P-256".to_string(),
            x: URL_SAFE_NO_PAD.encode(&bytes[1..33]),
            y: URL_SAFE_NO_PAD.encode(&bytes[33..65]),
            kid: kid.map(str::to_string),
        }
    }

    fn sign(key: &SigningKey, input: &[u8]) -> Vec<u8> {
        let digest = Sha256::digest(input);
        let sig: Signature = key.sign_prehash(&digest).unwrap();
        let sig = sig.normalize_s().unwrap_or(sig);
        sig.to_vec()
    }

    const INPUT: &[u8] = b"eyJ0eXAiOiJhdHByb3RvLWNsaWVudC1hdHRlc3RhdGlvbitqd3QifQ.eyJpc3MiOiJodHRwczovL2FwcC5leGFtcGxlLmNvbSJ9";

    #[test]
    fn verify_roundtrip() {
        let key = signing_key();
        let jwk = jwk_for(&key, Some("key-1"));
        let sig = sign(&key, INPUT);
        verify_es256(&jwk, INPUT, &sig).unwrap();
    }

    #[test]
    fn wrong_key_rejected() {
        let sig = sign(&signing_key(), INPUT);
        let other = SigningKey::from_slice(&[0x62u8; 32]).unwrap();
        let jwk = jwk_for(&other, None);
        assert!(matches!(
            verify_es256(&jwk, INPUT, &sig),
            Err(SpaceError::BadSignature)
        ));
    }

    #[test]
    fn tampered_input_rejected() {
        let key = signing_key();
        let jwk = jwk_for(&key, None);
        let sig = sign(&key, INPUT);
        let mut tampered = INPUT.to_vec();
        tampered[0] ^= 0xFF;
        assert!(matches!(
            verify_es256(&jwk, &tampered, &sig),
            Err(SpaceError::BadSignature)
        ));
    }

    #[test]
    fn json_roundtrip_and_validation() {
        let key = signing_key();
        let jwk = jwk_for(&key, Some("key-1"));
        let json = serde_json::to_string(&jwk).unwrap();
        assert_eq!(EcJwk::from_json(&json).unwrap(), jwk);

        let no_kid = serde_json::to_string(&jwk_for(&key, None)).unwrap();
        assert!(!no_kid.contains("kid"));

        assert!(matches!(
            EcJwk::from_json("{not json"),
            Err(SpaceError::Json(_))
        ));
    }

    #[test]
    fn wrong_kty_rejected() {
        let mut jwk = jwk_for(&signing_key(), None);
        jwk.kty = "RSA".to_string();
        let json = serde_json::to_string(&jwk).unwrap();
        assert!(matches!(
            EcJwk::from_json(&json),
            Err(SpaceError::InvalidJwk(msg)) if msg.contains("kty")
        ));
        assert!(matches!(
            verify_es256(&jwk, INPUT, &[0u8; 64]),
            Err(SpaceError::InvalidJwk(_))
        ));
    }

    #[test]
    fn wrong_crv_rejected() {
        let mut jwk = jwk_for(&signing_key(), None);
        jwk.crv = "P-384".to_string();
        let json = serde_json::to_string(&jwk).unwrap();
        assert!(matches!(
            EcJwk::from_json(&json),
            Err(SpaceError::InvalidJwk(msg)) if msg.contains("crv")
        ));
    }

    #[test]
    fn malformed_base64url_rejected() {
        let mut jwk = jwk_for(&signing_key(), None);
        jwk.x = "!!!not-base64url!!!".to_string();
        assert!(matches!(
            verify_es256(&jwk, INPUT, &[0u8; 64]),
            Err(SpaceError::InvalidJwk(msg)) if msg.contains("base64url")
        ));
    }

    #[test]
    fn wrong_coordinate_length_rejected() {
        let mut jwk = jwk_for(&signing_key(), None);
        jwk.y = URL_SAFE_NO_PAD.encode([0u8; 16]);
        assert!(matches!(
            verify_es256(&jwk, INPUT, &[0u8; 64]),
            Err(SpaceError::InvalidJwk(msg)) if msg.contains("32 bytes")
        ));
    }

    #[test]
    fn invalid_point_rejected() {
        let jwk = EcJwk {
            kty: "EC".to_string(),
            crv: "P-256".to_string(),
            x: URL_SAFE_NO_PAD.encode([0u8; 32]),
            y: URL_SAFE_NO_PAD.encode([0u8; 32]),
            kid: None,
        };
        let key = signing_key();
        let sig = sign(&key, INPUT);
        assert!(matches!(
            verify_es256(&jwk, INPUT, &sig),
            Err(SpaceError::Crypto(_))
        ));
    }

    #[test]
    fn jwk_set_find_by_kid() {
        let key = signing_key();
        let set = JwkSet {
            keys: vec![
                jwk_for(&key, None),
                jwk_for(&key, Some("key-1")),
                jwk_for(&key, Some("key-2")),
            ],
        };
        let json = serde_json::to_string(&set).unwrap();
        let set = JwkSet::from_json(&json).unwrap();
        assert_eq!(set.find("key-2").unwrap().kid.as_deref(), Some("key-2"));
        assert!(set.find("missing").is_none());
    }
}
