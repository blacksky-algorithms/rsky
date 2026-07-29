use crate::constants::{P256_JWT_ALG, PLUGINS, SECP256K1_JWT_ALG};
use crate::did::parse_did_key;
use crate::types::VerifyOptions;
use anyhow::{bail, Result};

pub fn verify_signature(
    did_key: &String,
    data: &[u8],
    sig: &[u8],
    opts: Option<VerifyOptions>,
) -> Result<bool> {
    let parsed = parse_did_key(did_key)?;
    let plugin = PLUGINS.into_iter().find(|p| p.jwt_alg == parsed.jwt_alg);
    match plugin {
        None => bail!("Unsupported signature alg: {0}", parsed.jwt_alg),
        Some(plugin) => (plugin.verify_signature)(did_key, data, sig, opts),
    }
}

/// Verify a signature over a caller-computed 32-byte sha256 digest.
///
/// The per-curve plugins behind [`verify_signature`] disagree on what `data`
/// means: secp256k1 treats it as the digest, p256 hashes it internally. This
/// entrypoint gives both curves digest semantics.
pub fn verify_signature_digest(
    did_key: &String,
    digest: &[u8],
    sig: &[u8],
    opts: Option<VerifyOptions>,
) -> Result<bool> {
    let parsed = parse_did_key(did_key)?;
    match parsed.jwt_alg {
        alg if alg == SECP256K1_JWT_ALG => {
            crate::secp256k1::operations::verify_did_sig(did_key, digest, sig, opts)
        }
        alg if alg == P256_JWT_ALG => {
            crate::p256::operations::verify_did_sig_prehash(did_key, digest, sig, opts)
        }
        alg => bail!("Unsupported signature alg: {alg}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::{P256_JWT_ALG, SECP256K1_JWT_ALG};
    use crate::did::format_did_key;
    use sha2::{Digest, Sha256};

    const MSG: &[u8] = b"test message for digest verification";

    fn secp256k1_fixture() -> (String, Vec<u8>, Vec<u8>) {
        use secp256k1::{Message, PublicKey, Secp256k1, SecretKey};
        let secret = SecretKey::from_slice(&[0x42u8; 32]).unwrap();
        let secp = Secp256k1::new();
        let pubkey = PublicKey::from_secret_key(&secp, &secret);
        let did = format_did_key(
            SECP256K1_JWT_ALG.to_string(),
            pubkey.serialize_uncompressed().to_vec(),
        )
        .unwrap();
        let digest = Sha256::digest(MSG);
        let msg = Message::from_digest_slice(&digest).unwrap();
        let mut sig = secret.sign_ecdsa(msg);
        sig.normalize_s();
        (did, digest.to_vec(), sig.serialize_compact().to_vec())
    }

    fn p256_fixture() -> (String, Vec<u8>, Vec<u8>) {
        use p256::ecdsa::signature::hazmat::PrehashSigner;
        use p256::ecdsa::{Signature, SigningKey};
        let signing_key = SigningKey::from_slice(&[0x42u8; 32]).unwrap();
        let did = format_did_key(
            P256_JWT_ALG.to_string(),
            signing_key
                .verifying_key()
                .to_encoded_point(true)
                .as_bytes()
                .to_vec(),
        )
        .unwrap();
        let digest = Sha256::digest(MSG);
        let sig: Signature = signing_key.sign_prehash(&digest).unwrap();
        let sig = sig.normalize_s().unwrap_or(sig);
        (did, digest.to_vec(), sig.to_vec())
    }

    #[test]
    fn digest_verify_secp256k1() {
        let (did, digest, sig) = secp256k1_fixture();
        assert!(verify_signature_digest(&did, &digest, &sig, None).unwrap());
        let wrong = Sha256::digest(b"other message");
        assert!(!verify_signature_digest(&did, &wrong, &sig, None).unwrap());
    }

    #[test]
    fn digest_verify_p256() {
        let (did, digest, sig) = p256_fixture();
        assert!(verify_signature_digest(&did, &digest, &sig, None).unwrap());
        let wrong = Sha256::digest(b"other message");
        assert!(!verify_signature_digest(&did, &wrong, &sig, None).unwrap());
    }

    #[test]
    fn p256_high_s_signature_rejected() {
        use p256::ecdsa::signature::hazmat::PrehashSigner;
        use p256::ecdsa::{Signature, SigningKey};
        let signing_key = SigningKey::from_slice(&[0x42u8; 32]).unwrap();
        let digest = Sha256::digest(MSG);
        let sig: Signature = signing_key.sign_prehash(&digest).unwrap();
        let low = sig.normalize_s().unwrap_or(sig);
        // Denormalize: -s mod n is the high-S counterpart of a low-S sig.
        let high = {
            let (r, s) = (low.r(), low.s());
            Signature::from_scalars(*r.as_ref(), -*s.as_ref()).unwrap()
        };
        let did = p256_fixture().0;
        assert!(!verify_signature_digest(&did, &digest, &high.to_vec(), None).unwrap());
    }

    #[test]
    fn digest_verify_matches_raw_p256_semantics() {
        // The legacy entrypoint hashes internally for p256; the digest
        // entrypoint must accept the caller-hashed equivalent.
        let (did, digest, sig) = p256_fixture();
        assert!(verify_signature(&did, MSG, &sig, None).unwrap());
        assert!(verify_signature_digest(&did, &digest, &sig, None).unwrap());
    }
}
