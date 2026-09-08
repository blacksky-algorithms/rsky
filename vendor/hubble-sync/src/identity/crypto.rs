//! atproto signature verification
//!
//! use [`super::SigningKey`], which delegates to the right algorithm from here.
//!
//! signatures not in Low-S form are rejected
//!
//! sha256 over the message (described in atproto spec) is already done by k256/
//! p256 libraries.

#[derive(Debug, thiserror::Error)]
pub enum SignatureError {
    #[error("Invalid key")]
    BadKey,
    #[error("Malformed signature")]
    BadSignature,
    #[error("ECDSA signature not in Low-S form")]
    HighS,
    #[error("Signature did not verify with this key")]
    VerificationFailed,
}

impl SignatureError {
    pub fn could_retry_with_another_key(&self) -> bool {
        match self {
            Self::BadKey => true,
            Self::BadSignature => true, // signature construction depends on key type
            Self::HighS => false,       // problem with the actual signature
            Self::VerificationFailed => true,
        }
    }
}

/// verify the signature over a message with a secp256k1 key
///
/// msg is the encoded bytes -- k256 does the sha256 hashing for us
pub fn verify_k256_ecdsa(key: &[u8], msg: &[u8], sig: &[u8]) -> Result<(), SignatureError> {
    use SignatureError as E;
    use k256::ecdsa::{Signature, VerifyingKey, signature::Verifier};
    use k256::elliptic_curve::scalar::IsHigh;

    let vk = VerifyingKey::from_sec1_bytes(key).map_err(|_| E::BadKey)?;
    let sig = Signature::from_slice(sig).map_err(|_| E::BadSignature)?;

    // atproto requiers signatures to be in Low-S form
    // https://www.ietf.org/archive/id/draft-holmgren-at-repository-02.html#appendix-B.1
    if sig.s().is_high().into() {
        return Err(E::HighS);
    }

    vk.verify(msg, &sig).map_err(|_| E::VerificationFailed)
}

/// verify the signature over a message with a p256 key
///
/// msg is the encoded bytes -- p256 does the sha256 hashing for us
pub fn verify_p256_ecdsa(key: &[u8], msg: &[u8], sig: &[u8]) -> Result<(), SignatureError> {
    use SignatureError as E;
    use p256::ecdsa::{Signature, VerifyingKey, signature::Verifier};
    use p256::elliptic_curve::scalar::IsHigh;

    let vk = VerifyingKey::from_sec1_bytes(key).map_err(|_| E::BadKey)?;
    let sig = Signature::from_slice(sig).map_err(|_| E::BadSignature)?;

    // atproto requiers signatures to be in Low-S form
    // https://www.ietf.org/archive/id/draft-holmgren-at-repository-02.html#appendix-B.1
    if sig.s().is_high().into() {
        return Err(E::HighS);
    }

    vk.verify(msg, &sig).map_err(|_| E::VerificationFailed)
}
