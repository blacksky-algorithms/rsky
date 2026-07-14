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
    let prefixed_bytes = extract_prefixed_bytes(extract_multikey(did)?)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn fixture() -> (secp256k1::SecretKey, PublicKey, Vec<u8>, ecdsa::Signature) {
        let secret = secp256k1::SecretKey::from_slice(&[0x17u8; 32]).unwrap();
        let pubkey = PublicKey::from_secret_key(&Secp256k1::new(), &secret);
        let digest = Sha256::digest(b"msg").to_vec();
        let msg = Message::from_digest_slice(&digest).unwrap();
        let mut sig = secret.sign_ecdsa(msg);
        sig.normalize_s();
        (secret, pubkey, digest, sig)
    }

    #[test]
    fn rejects_non_secp256k1_did_key() {
        use p256::ecdsa::SigningKey;
        let p256_key = SigningKey::from_slice(&[0x17u8; 32]).unwrap();
        let p256_did = crate::did::format_did_key(
            crate::constants::P256_JWT_ALG.to_string(),
            p256_key
                .verifying_key()
                .to_encoded_point(false)
                .as_bytes()
                .to_vec(),
        )
        .unwrap();
        let (_, _, digest, sig) = fixture();
        assert!(verify_did_sig(&p256_did, &digest, &sig.serialize_compact(), None).is_err());
    }

    #[test]
    fn der_signature_requires_malleable_option() {
        let (_, pubkey, digest, sig) = fixture();
        let der = sig.serialize_der().to_vec();
        let opts = Some(VerifyOptions {
            allow_malleable_sig: Some(true),
        });
        assert!(!verify_sig(&pubkey.serialize(), &digest, &der, None).unwrap());
        assert!(verify_sig(&pubkey.serialize(), &digest, &der, opts).unwrap());
    }

    #[test]
    fn compact_format_rejects_garbage() {
        assert!(!is_compact_format(&[0u8; 10]));
    }
}
