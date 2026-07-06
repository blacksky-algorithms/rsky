//! secp256k1 signing for the space authority.
//!
//! The authority owns the space's `#atproto_space` signing key and uses it to
//! mint space credentials. Signing follows the atproto convention: sha256 the
//! message, ECDSA-sign, normalize to low-S, serialize compact (r||s).

use secp256k1::{Message, PublicKey, Secp256k1, SecretKey};
use sha2::{Digest, Sha256};

use crate::error::HostError;

/// Holds the authority's secret key and derives its `did:key`.
#[derive(Clone)]
pub struct Signer {
    secret: SecretKey,
    did_key: String,
}

impl Signer {
    /// Load from a hex-encoded 32-byte secp256k1 secret key.
    pub fn from_hex(hex_key: &str) -> Result<Self, HostError> {
        let bytes = hex::decode(hex_key.trim()).map_err(|e| HostError::Key(e.to_string()))?;
        let secret = SecretKey::from_slice(&bytes).map_err(|e| HostError::Key(e.to_string()))?;
        Ok(Self::from_secret(secret))
    }

    pub fn from_secret(secret: SecretKey) -> Self {
        let secp = Secp256k1::new();
        let pubkey = PublicKey::from_secret_key(&secp, &secret);
        let did_key = rsky_crypto::utils::encode_did_key(&pubkey);
        Self { secret, did_key }
    }

    /// The authority's `did:key`, e.g. published as `#atproto_space`.
    pub fn did_key(&self) -> &str {
        &self.did_key
    }

    /// Sign arbitrary bytes (used for the JWT signing input).
    pub fn sign(&self, input: &[u8]) -> Result<Vec<u8>, String> {
        let hash = Sha256::digest(input);
        let msg = Message::from_digest_slice(hash.as_ref()).map_err(|e| e.to_string())?;
        let mut sig = self.secret.sign_ecdsa(msg);
        sig.normalize_s();
        Ok(sig.serialize_compact().to_vec())
    }
}

#[cfg(test)]
pub(crate) fn test_signer() -> Signer {
    // Deterministic non-zero test key.
    let secret = SecretKey::from_slice(&[0x11u8; 32]).unwrap();
    Signer::from_secret(secret)
}
