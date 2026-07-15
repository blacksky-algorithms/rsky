//! Serve-time signing of permissioned-repo commits.
//!
//! A commit is produced fresh for every reader it is served to: a new random
//! `ikm` each time, a signature over the domain-separated context (never over
//! the repo hash), and a MAC binding the hash to the context. See
//! `rsky_space::commit` for the verification side and the deniability
//! rationale.

use anyhow::Result;
use rand::RngCore;
use rsky_space::commit::{build_ctx, compute_mac};
use rsky_space::types::SignedCommit;
use secp256k1::{Keypair, Message};
use serde_bytes::ByteBuf;
use sha2::{Digest, Sha256};

pub const COMMIT_VERSION: u8 = 1;

/// Sign a commit for one serving: fresh 32-byte `ikm`, `sig = sign(sha256(ctx))`
/// with the actor's repo signing key (low-S compact), and
/// `mac = HMAC-SHA256(HKDF-SHA256(ikm, ctx), hash)`.
pub fn sign_commit(
    keypair: &Keypair,
    space_uri: &str,
    author_did: &str,
    rev: &str,
    hash: &[u8; 32],
) -> Result<SignedCommit> {
    let mut ikm = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut ikm);
    sign_commit_with_ikm(keypair, space_uri, author_did, rev, hash, ikm)
}

fn sign_commit_with_ikm(
    keypair: &Keypair,
    space_uri: &str,
    author_did: &str,
    rev: &str,
    hash: &[u8; 32],
    ikm: [u8; 32],
) -> Result<SignedCommit> {
    let ctx = build_ctx(space_uri, author_did, rev, &ikm);
    let digest = Sha256::digest(&ctx);
    let message = Message::from_digest_slice(&digest)?;
    let mut sig = keypair.secret_key().sign_ecdsa(message);
    sig.normalize_s();
    let mac = compute_mac(&ikm, &ctx, hash).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(SignedCommit {
        ver: COMMIT_VERSION,
        hash: ByteBuf::from(hash.to_vec()),
        ikm: ByteBuf::from(ikm.to_vec()),
        sig: ByteBuf::from(sig.serialize_compact().to_vec()),
        mac: ByteBuf::from(mac.to_vec()),
        rev: rev.to_string(),
    })
}

/// The wire (`$bytes`) representation used by the XRPC outputs.
pub fn to_lexicon(commit: &SignedCommit) -> rsky_lexicon::com::atproto::space::SignedCommit {
    rsky_lexicon::com::atproto::space::SignedCommit {
        ver: commit.ver as i64,
        hash: commit.hash.to_vec(),
        ikm: commit.ikm.to_vec(),
        sig: commit.sig.to_vec(),
        mac: commit.mac.to_vec(),
        rev: commit.rev.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsky_crypto::utils::encode_did_key;
    use rsky_space::commit::verify_commit;
    use secp256k1::{Secp256k1, SecretKey};

    const SPACE: &str = "at://did:plc:auth/space/com.example.forum/self";
    const AUTHOR: &str = "did:plc:author";
    const REV: &str = "3jzfcijpj2z2a";

    fn keypair() -> Keypair {
        let secp = Secp256k1::new();
        Keypair::from_secret_key(&secp, &SecretKey::from_slice(&[0x33u8; 32]).unwrap())
    }

    #[test]
    fn signed_commit_verifies_end_to_end() {
        let keypair = keypair();
        let did_key = encode_did_key(&keypair.public_key());
        let hash = [7u8; 32];
        let commit = sign_commit(&keypair, SPACE, AUTHOR, REV, &hash).unwrap();
        assert_eq!(commit.ver, COMMIT_VERSION);
        assert_eq!(commit.rev, REV);
        verify_commit(
            &did_key,
            SPACE,
            AUTHOR,
            &commit.rev,
            &commit.ikm,
            &commit.sig,
            &commit.mac,
            &commit.hash,
        )
        .expect("serve-time commit must verify");
    }

    #[test]
    fn each_serving_gets_a_fresh_ikm() {
        let keypair = keypair();
        let hash = [7u8; 32];
        let a = sign_commit(&keypair, SPACE, AUTHOR, REV, &hash).unwrap();
        let b = sign_commit(&keypair, SPACE, AUTHOR, REV, &hash).unwrap();
        assert_ne!(a.ikm, b.ikm);
        assert_ne!(a.sig, b.sig);
    }

    #[test]
    fn tampered_fields_fail_verification() {
        let keypair = keypair();
        let did_key = encode_did_key(&keypair.public_key());
        let hash = [7u8; 32];
        let commit = sign_commit_with_ikm(&keypair, SPACE, AUTHOR, REV, &hash, [9u8; 32]).unwrap();
        // Wrong hash breaks the MAC.
        assert!(verify_commit(
            &did_key,
            SPACE,
            AUTHOR,
            &commit.rev,
            &commit.ikm,
            &commit.sig,
            &commit.mac,
            &[8u8; 32],
        )
        .is_err());
        // Wrong rev breaks the signature context.
        assert!(verify_commit(
            &did_key,
            SPACE,
            AUTHOR,
            "3jzfcijpj2z2b",
            &commit.ikm,
            &commit.sig,
            &commit.mac,
            &commit.hash,
        )
        .is_err());
        // A different author's key rejects.
        let secp = Secp256k1::new();
        let other = Keypair::from_secret_key(&secp, &SecretKey::from_slice(&[0x44u8; 32]).unwrap());
        assert!(verify_commit(
            &encode_did_key(&other.public_key()),
            SPACE,
            AUTHOR,
            &commit.rev,
            &commit.ikm,
            &commit.sig,
            &commit.mac,
            &commit.hash,
        )
        .is_err());
    }

    #[test]
    fn lexicon_conversion_is_field_faithful() {
        let keypair = keypair();
        let hash = [7u8; 32];
        let commit = sign_commit(&keypair, SPACE, AUTHOR, REV, &hash).unwrap();
        let wire = to_lexicon(&commit);
        assert_eq!(wire.ver, 1);
        assert_eq!(wire.hash, commit.hash.to_vec());
        assert_eq!(wire.ikm, commit.ikm.to_vec());
        assert_eq!(wire.sig, commit.sig.to_vec());
        assert_eq!(wire.mac, commit.mac.to_vec());
        assert_eq!(wire.rev, commit.rev);
    }
}
