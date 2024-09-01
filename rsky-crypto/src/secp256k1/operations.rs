use crate::constants::SECP256K1_DID_PREFIX;
use crate::types::VerifyOptions;
use crate::utils::{extract_multikey, extract_prefixed_bytes, has_prefix};
use anyhow::{bail, Result};
use secp256k1::{ecdsa, Message, PublicKey, Secp256k1};

pub fn verify_did_sig(
    did: &String,
    data: &[u8],
    sig: &[u8],
    opts: Option<VerifyOptions>,
) -> Result<bool> {
    let prefixed_bytes = extract_prefixed_bytes(extract_multikey(&did)?)?;
    if !has_prefix(&prefixed_bytes, &SECP256K1_DID_PREFIX.to_vec()) {
        bail!("Not a secp256k1 did:key: {did}");
    }
    let key_bytes = &prefixed_bytes[SECP256K1_DID_PREFIX.len()..];
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
    let is_compact = is_compact_format(sig);
    if !allow_malleable && !is_compact {
        return Ok(false);
    }
    let secp = Secp256k1::verification_only();
    let public_key = PublicKey::from_slice(public_key)?;
    let data = Message::from_digest_slice(data)?;
    let sig = match is_compact {
        true => ecdsa::Signature::from_compact(sig)?,
        false => ecdsa::Signature::from_der(sig)?,
    };
    Ok(secp.verify_ecdsa(&data, &sig, &public_key).is_ok())
}

pub fn is_compact_format(sig: &[u8]) -> bool {
    match ecdsa::Signature::from_compact(sig) {
        Ok(parsed) => parsed.serialize_compact() == sig,
        Err(_) => false,
    }
}
