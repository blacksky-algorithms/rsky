use crate::constants::P256_DID_PREFIX;
use crate::types::VerifyOptions;
use crate::utils::{extract_multikey, extract_prefixed_bytes, has_prefix};
use anyhow::{bail, Result};
use p256::ecdsa::signature::hazmat::PrehashVerifier;
use p256::ecdsa::{signature::Verifier, Signature, VerifyingKey};

pub fn verify_did_sig(
    did: &String,
    data: &[u8],
    sig: &[u8],
    opts: Option<VerifyOptions>,
) -> Result<bool> {
    let prefixed_bytes = extract_prefixed_bytes(extract_multikey(did)?)?;
    if !has_prefix(&prefixed_bytes, &P256_DID_PREFIX.to_vec()) {
        bail!("Not a P-256 did:key: {did}");
    }
    let key_bytes = &prefixed_bytes[P256_DID_PREFIX.len()..];
    verify_sig(key_bytes, data, sig, opts)
}

pub fn verify_sig(
    public_key: &[u8],
    data: &[u8],
    sig: &[u8],
    opts: Option<VerifyOptions>,
) -> Result<bool> {
    let allow_malleable = match opts {
        Some(opts) if opts.allow_malleable_sig.is_some() => opts.allow_malleable_sig.unwrap(),
        _ => false,
    };
    if !allow_malleable && !is_compact_format(sig) {
        return Ok(false);
    }
    let verifying_key = VerifyingKey::from_sec1_bytes(public_key)?;
    let signature = Signature::try_from(sig)?;
    Ok(verifying_key.verify(data, &signature).is_ok())
}

pub fn verify_did_sig_prehash(
    did: &String,
    digest: &[u8],
    sig: &[u8],
    opts: Option<VerifyOptions>,
) -> Result<bool> {
    let prefixed_bytes = extract_prefixed_bytes(extract_multikey(did)?)?;
    if !has_prefix(&prefixed_bytes, &P256_DID_PREFIX.to_vec()) {
        bail!("Not a P-256 did:key: {did}");
    }
    let key_bytes = &prefixed_bytes[P256_DID_PREFIX.len()..];
    verify_prehash_sig(key_bytes, digest, sig, opts)
}

pub fn verify_prehash_sig(
    public_key: &[u8],
    digest: &[u8],
    sig: &[u8],
    opts: Option<VerifyOptions>,
) -> Result<bool> {
    let allow_malleable = match opts {
        Some(opts) if opts.allow_malleable_sig.is_some() => opts.allow_malleable_sig.unwrap(),
        _ => false,
    };
    if !allow_malleable && !is_compact_format(sig) {
        return Ok(false);
    }
    let verifying_key = VerifyingKey::from_sec1_bytes(public_key)?;
    let signature = Signature::try_from(sig)?;
    Ok(verifying_key.verify_prehash(digest, &signature).is_ok())
}

pub fn is_compact_format(sig: &[u8]) -> bool {
    // Fixed-size (r||s) encoding that is already low-S: `normalize_s` returns
    // None exactly when `s` is in the lower half of the curve order.
    match Signature::try_from(sig) {
        Ok(parsed) => parsed.normalize_s().is_none(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::signature::hazmat::PrehashSigner;
    use p256::ecdsa::SigningKey;
    use sha2::{Digest, Sha256};

    fn fixture() -> (SigningKey, Vec<u8>, Vec<u8>) {
        let key = SigningKey::from_slice(&[0x16u8; 32]).unwrap();
        let digest = Sha256::digest(b"msg").to_vec();
        let sig: Signature = key.sign_prehash(&digest).unwrap();
        let sig = sig.normalize_s().unwrap_or(sig);
        (key, digest, sig.to_vec())
    }

    #[test]
    fn rejects_non_p256_did_key() {
        use secp256k1::{PublicKey, Secp256k1, SecretKey};
        let secret = SecretKey::from_slice(&[0x16u8; 32]).unwrap();
        let k256_did =
            crate::utils::encode_did_key(&PublicKey::from_secret_key(&Secp256k1::new(), &secret));
        let (_, digest, sig) = fixture();
        assert!(verify_did_sig(&k256_did, &digest, &sig, None).is_err());
        assert!(verify_did_sig_prehash(&k256_did, &digest, &sig, None).is_err());
    }

    #[test]
    fn malleable_option_skips_low_s_gate() {
        let (key, digest, _) = fixture();
        let sig: Signature = key.sign_prehash(&digest).unwrap();
        let low = sig.normalize_s().unwrap_or(sig);
        // -s mod n is the high-S counterpart of a low-S signature.
        let high = Signature::from_scalars(*low.r().as_ref(), -*low.s().as_ref()).unwrap();
        let point = key.verifying_key().to_encoded_point(false);
        let opts = || {
            Some(VerifyOptions {
                allow_malleable_sig: Some(true),
            })
        };
        assert!(!verify_prehash_sig(point.as_bytes(), &digest, &high.to_vec(), None).unwrap());
        assert!(verify_prehash_sig(point.as_bytes(), &digest, &high.to_vec(), opts()).unwrap());
        // Same gate on the raw-message path.
        assert!(!verify_sig(point.as_bytes(), b"msg", &high.to_vec(), None).unwrap());
        assert!(verify_sig(point.as_bytes(), b"msg", &high.to_vec(), opts()).unwrap());
    }

    #[test]
    fn compact_format_rejects_garbage() {
        assert!(!is_compact_format(&[0u8; 10]));
    }
}
