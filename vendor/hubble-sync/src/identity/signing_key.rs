//! raw bytes for atproto multibase signing keys

use std::fmt;
use std::str::FromStr;

use super::crypto::{SignatureError, verify_k256_ecdsa, verify_p256_ecdsa};
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum MultibaseError {
    #[error("Wrong base. expected Base58Btc, got {0:?}")]
    WrongBase(multibase::Base),
    #[error("Multibase error: {0}")]
    Fail(String),
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SigningKey(Vec<u8>);

/// a signing key that can verify stuff
///
/// accepts any validly-encoded content, including invalid keys
///
/// - invalid keys will fail when trying to decode anything
/// - unsupported codecs will fail when trying to verify anything
///
/// allowing some invalid content means we don't fail identity resolution for
/// it, and means we won't be missing data if support for  new key codecs are
/// added to atproto in the future.
impl SigningKey {
    pub fn raw(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
    pub fn decode(s: &str) -> Result<Self, MultibaseError> {
        let (base, decoded) = multibase::decode(s)
            .map_err(|e| MultibaseError::Fail(format!("invalid multibase key: {e}")))?;
        if base != multibase::Base::Base58Btc {
            return Err(MultibaseError::WrongBase(base));
        }
        Ok(Self(decoded))
    }
    pub fn encode(&self) -> String {
        multibase::encode(multibase::Base::Base58Btc, &self.0)
    }

    /// atproto signatures over bytes
    ///
    /// pass the encoded bytes directly -- sha256 is applied by the crypto
    /// libraries.
    pub fn verify_bytes(&self, bytes: &[u8], signature: &[u8]) -> Result<(), SignatureError> {
        // canonical unsigned-varint multicodec prefixes for the only two key
        // types atproto supports.
        // secp256k1-pub = 0xE7, p256-pub = 0x1200
        const K256_MULTICODEC: [u8; 2] = [0xE7, 0x01];
        const P256_MULTICODEC: [u8; 2] = [0x80, 0x24];

        if let Some(key) = self.0.strip_prefix(&K256_MULTICODEC) {
            verify_k256_ecdsa(key, bytes, signature)
        } else if let Some(key) = self.0.strip_prefix(&P256_MULTICODEC) {
            verify_p256_ecdsa(key, bytes, signature)
        } else {
            Err(SignatureError::BadKey)
        }
    }
}

impl fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("SigningKey").field(&self.encode()).finish()
    }
}

impl fmt::Display for SigningKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", &self.encode())
    }
}

impl FromStr for SigningKey {
    type Err = MultibaseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::decode(s)
    }
}
