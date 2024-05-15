use crate::constants::P256_DID_PREFIX;
use crate::types::VerifyOptions;
use crate::utils::{extract_multikey, extract_prefixed_bytes, has_prefix};
use anyhow::{bail, Result};
use p256::ecdsa::{signature::Verifier, Signature, VerifyingKey};

pub fn verify_did_sig(
    did: &String,
    data: &[u8],
    sig: &[u8],
    opts: Option<VerifyOptions>,
) -> Result<bool> {
    let prefixed_bytes = extract_prefixed_bytes(extract_multikey(&did)?)?;
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

pub fn is_compact_format(sig: &[u8]) -> bool {
    let mut parsed = match Signature::try_from(sig) {
        Ok(res) => res,
        Err(_) => return false,
    };
    parsed = match parsed.normalize_s() {
        Some(res) => res,
        None => return false,
    };
    parsed.to_vec() == *sig
}
