//! Minimal EC JWK support for verifying ES256- and ES256K-signed JWTs
//! (proposal §Client attestation).
//!
//! A space authority verifies a client attestation by resolving the client's
//! published JWKS and checking the JWT signature against the key named by the
//! attestation's `kid`. Only `kty: "EC"` keys on the `P-256` and `secp256k1`
//! curves are supported. Each verifier requires its own curve, so a key can
//! never be verified under an algorithm it was not issued for.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Result, SpaceError};

const COORD_LEN: usize = 32;

/// The JWK `crv` of an ES256 key.
pub const CRV_P256: &str = "P-256";
/// The JWK `crv` of an ES256K key.
pub const CRV_SECP256K1: &str = "secp256k1";

/// An EC public JWK on a supported curve.
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
        if self.crv != CRV_P256 && self.crv != CRV_SECP256K1 {
            return Err(SpaceError::InvalidJwk(format!(
                "unsupported crv: {}",
                self.crv
            )));
        }
        Ok(())
    }

    /// Validate the key and require one specific curve.
    pub fn require_crv(&self, crv: &str) -> Result<()> {
        self.validate()?;
        if self.crv != crv {
            return Err(SpaceError::InvalidJwk(format!(
                "expected crv {crv}, got {}",
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
    jwk.require_crv(CRV_P256)?;
    let point = jwk.sec1_point()?;
    let ok = rsky_crypto::p256::operations::verify_sig(&point, signing_input, sig, None)
        .map_err(|e| SpaceError::Crypto(e.to_string()))?;
    if ok {
        Ok(())
    } else {
        Err(SpaceError::BadSignature)
    }
}

/// Verify an ES256K signature over a JWT signing input
/// (`header_b64.payload_b64` bytes) against a secp256k1 JWK.
///
/// `sig` must be the compact 64-byte low-S `r || s` encoding; anything else is
/// rejected as a bad signature.
pub fn verify_es256k(jwk: &EcJwk, signing_input: &[u8], sig: &[u8]) -> Result<()> {
    jwk.require_crv(CRV_SECP256K1)?;
    let point = jwk.sec1_point()?;
    let digest = Sha256::digest(signing_input);
    let ok = rsky_crypto::secp256k1::operations::verify_sig(&point, &digest, sig, None)
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

    fn k1_key() -> secp256k1::SecretKey {
        secp256k1::SecretKey::from_slice(&[0x63u8; 32]).unwrap()
    }

    fn k1_jwk_for(key: &secp256k1::SecretKey) -> EcJwk {
        let secp = secp256k1::Secp256k1::new();
        let point = key.public_key(&secp).serialize_uncompressed();
        EcJwk {
            kty: "EC".to_string(),
            crv: CRV_SECP256K1.to_string(),
            x: URL_SAFE_NO_PAD.encode(&point[1..33]),
            y: URL_SAFE_NO_PAD.encode(&point[33..65]),
            kid: None,
        }
    }

    fn k1_sign(key: &secp256k1::SecretKey, input: &[u8]) -> Vec<u8> {
        let secp = secp256k1::Secp256k1::new();
        let digest = Sha256::digest(input);
        let message = secp256k1::Message::from_digest_slice(&digest).unwrap();
        secp.sign_ecdsa(&message, key).serialize_compact().to_vec()
    }

    #[test]
    fn es256k_verify_roundtrip() {
        let key = k1_key();
        verify_es256k(&k1_jwk_for(&key), INPUT, &k1_sign(&key, INPUT)).unwrap();
    }

    #[test]
    fn es256k_wrong_key_rejected() {
        let sig = k1_sign(&k1_key(), INPUT);
        let other = secp256k1::SecretKey::from_slice(&[0x64u8; 32]).unwrap();
        assert!(matches!(
            verify_es256k(&k1_jwk_for(&other), INPUT, &sig),
            Err(SpaceError::BadSignature)
        ));
    }

    #[test]
    fn es256k_tampered_input_rejected() {
        let key = k1_key();
        let sig = k1_sign(&key, INPUT);
        let mut tampered = INPUT.to_vec();
        tampered[0] ^= 0xFF;
        assert!(matches!(
            verify_es256k(&k1_jwk_for(&key), &tampered, &sig),
            Err(SpaceError::BadSignature)
        ));
    }

    #[test]
    fn curve_and_algorithm_must_agree() {
        let k1 = k1_key();
        let k1_sig = k1_sign(&k1, INPUT);
        assert!(matches!(
            verify_es256(&k1_jwk_for(&k1), INPUT, &k1_sig),
            Err(SpaceError::InvalidJwk(msg)) if msg.contains("crv")
        ));

        let p = signing_key();
        let p_sig = sign(&p, INPUT);
        assert!(matches!(
            verify_es256k(&jwk_for(&p, None), INPUT, &p_sig),
            Err(SpaceError::InvalidJwk(msg)) if msg.contains("crv")
        ));
    }

    #[test]
    fn es256k_jwk_survives_json_roundtrip() {
        let jwk = k1_jwk_for(&k1_key());
        let json = serde_json::to_string(&jwk).unwrap();
        assert_eq!(EcJwk::from_json(&json).unwrap(), jwk);
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
