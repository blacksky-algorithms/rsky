//! Deniable commit context and verification (proposal §Commit signature).
//!
//! A user never signs the repo digest directly, since that would be a
//! rebroadcastable proof of private content. Instead the signature covers only
//! random per-commit bytes (`ikm`) bound into a domain-separated context
//! `ctx`, and the digest is bound to that context by a symmetric MAC. A reader
//! gets full authenticity + integrity, but a leaked commit proves nothing about
//! its contents because anyone holding the public `ikm` can forge a valid MAC
//! for any `hash`.

use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::error::{Result, SpaceError};

/// Fixed protocol tag; also names the commit format version (`ver = 1`).
const PROTO_TAG: &[u8] = b"atproto-space-v1";

/// The tag the spaces specification names for repo commits, which hosts
/// following it sign under (rsky-pds since `954becb4`). The rest of the
/// context is unchanged; only the tag differs. See [`verify_commit_either_tag`].
pub const REPO_PROTO_TAG: &[u8] = b"atproto-space-repo-v1";

type HmacSha256 = Hmac<Sha256>;

/// Build the domain-separation context:
///
/// ```text
/// ctx = "atproto-space-v1"
///    || u16be(len(space))  || space   // at://authority/space/type/skey
///    || u16be(len(author)) || author  // author DID of the repo
///    || u16be(len(rev))    || rev     // commit revision (TID)
///    || u16be(len(ikm))    || ikm     // per-signature nonce
/// ```
///
/// Length prefixes are big-endian (TLS §3.4 convention), the opposite byte
/// order from the little-endian LtHash lanes — the two come from different
/// specs and each keeps its native order.
pub fn build_ctx(space: &str, author: &str, rev: &str, ikm: &[u8]) -> Vec<u8> {
    build_ctx_with_tag(PROTO_TAG, space, author, rev, ikm)
}

/// [`build_ctx`] under a given protocol tag.
pub fn build_ctx_with_tag(tag: &[u8], space: &str, author: &str, rev: &str, ikm: &[u8]) -> Vec<u8> {
    let mut ctx =
        Vec::with_capacity(tag.len() + 8 + space.len() + author.len() + rev.len() + ikm.len());
    ctx.extend_from_slice(tag);
    for field in [space.as_bytes(), author.as_bytes(), rev.as_bytes(), ikm] {
        ctx.extend_from_slice(&(field.len() as u16).to_be_bytes());
        ctx.extend_from_slice(field);
    }
    ctx
}

/// Compute `mac = HMAC-SHA256(HKDF-SHA256(ikm, ctx), hash)`.
///
/// HKDF is expand-only: `ikm` is used directly as the PRK, with no extract
/// step and no salt. An extract step here produces a different key and a MAC
/// no other implementation can reproduce.
pub fn compute_mac(ikm: &[u8], ctx: &[u8], hash: &[u8]) -> Result<[u8; 32]> {
    let hk = Hkdf::<Sha256>::from_prk(ikm).map_err(|_| SpaceError::Hkdf)?;
    let mut okm = [0u8; 32];
    hk.expand(ctx, &mut okm).map_err(|_| SpaceError::Hkdf)?;
    let mut mac = <HmacSha256 as Mac>::new_from_slice(&okm).map_err(|_| SpaceError::Hkdf)?;
    mac.update(hash);
    Ok(mac.finalize().into_bytes().into())
}

/// Verify the MAC binding the repo `hash` to this commit's context.
pub fn verify_mac(ikm: &[u8], ctx: &[u8], hash: &[u8], mac: &[u8]) -> Result<()> {
    let expected = compute_mac(ikm, ctx, hash)?;
    if expected.ct_eq(mac).into() {
        Ok(())
    } else {
        Err(SpaceError::BadMac)
    }
}

/// Fully verify a served commit against the author's atproto signing key.
///
/// 1. Verify `sig` over `ctx` (authenticity). The signed message contains only
///    `(space, author, rev, ikm)`, never the repo hash.
/// 2. Recompute and compare the MAC (integrity), which trusts `hash`.
///
/// `did_key` is the author's `did:key`-encoded signing key (as resolved from
/// their DID document). `hash` is the digest the reader independently derived
/// by folding the repo's records into an [`crate::lthash::LtHash`].
#[allow(clippy::too_many_arguments)]
pub fn verify_commit(
    did_key: &str,
    space: &str,
    author: &str,
    rev: &str,
    ikm: &[u8],
    sig: &[u8],
    mac: &[u8],
    hash: &[u8],
) -> Result<()> {
    verify_commit_with_tag(PROTO_TAG, did_key, space, author, rev, ikm, sig, mac, hash)
}

/// [`verify_commit`], accepting a commit signed under either [`PROTO_TAG`]
/// or [`REPO_PROTO_TAG`].
///
/// For a syncer, which pulls from each member's own repo host and so meets
/// hosts on both sides of the tag change. The two contexts differ, so a
/// signature or MAC made under one never verifies under the other; accepting
/// both only accepts what one of the two protocol versions would.
#[allow(clippy::too_many_arguments)]
pub fn verify_commit_either_tag(
    did_key: &str,
    space: &str,
    author: &str,
    rev: &str,
    ikm: &[u8],
    sig: &[u8],
    mac: &[u8],
    hash: &[u8],
) -> Result<()> {
    verify_commit_with_tag(PROTO_TAG, did_key, space, author, rev, ikm, sig, mac, hash).or_else(
        |_| {
            verify_commit_with_tag(
                REPO_PROTO_TAG,
                did_key,
                space,
                author,
                rev,
                ikm,
                sig,
                mac,
                hash,
            )
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn verify_commit_with_tag(
    tag: &[u8],
    did_key: &str,
    space: &str,
    author: &str,
    rev: &str,
    ikm: &[u8],
    sig: &[u8],
    mac: &[u8],
    hash: &[u8],
) -> Result<()> {
    let ctx = build_ctx_with_tag(tag, space, author, rev, ikm);
    let digest = Sha256::digest(&ctx);
    let ok = rsky_crypto::verify::verify_signature_digest(&did_key.to_string(), &digest, sig, None)
        .map_err(|e| SpaceError::Crypto(e.to_string()))?;
    if !ok {
        return Err(SpaceError::BadSignature);
    }
    verify_mac(ikm, &ctx, hash, mac)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_tag_mac_matches_the_spec_vector() {
        // The vector rsky-pds pins for `atproto-space-repo-v1` (after
        // `954becb4`), so this side derives exactly what those hosts sign.
        let ikm = [7u8; 32];
        let ctx = build_ctx_with_tag(
            REPO_PROTO_TAG,
            "at://a/space/t/main",
            "did:plc:author",
            "3krev",
            &ikm,
        );
        let mac = compute_mac(&ikm, &ctx, &[42u8; 32]).unwrap();
        assert_eq!(
            hex::encode(mac),
            "88d7d33cf56a68b7be6757dfb8851db24403e3fb56bfeb647229b2c8c7cf501c"
        );
    }

    #[test]
    fn either_tag_verifies_commits_signed_under_both() {
        use secp256k1::{Message, PublicKey, Secp256k1, SecretKey};
        let secret = SecretKey::from_slice(&[0x22u8; 32]).unwrap();
        let did_key = rsky_crypto::utils::encode_did_key(&PublicKey::from_secret_key(
            &Secp256k1::new(),
            &secret,
        ));
        let (space, author, rev, ikm, hash) = (
            "at://a/space/t/main",
            "did:plc:a",
            "3r",
            [5u8; 32],
            [6u8; 32],
        );
        let sign = |tag: &[u8]| {
            let ctx = build_ctx_with_tag(tag, space, author, rev, &ikm);
            let msg = Message::from_digest_slice(&Sha256::digest(&ctx)).unwrap();
            let mut sig = secret.sign_ecdsa(msg);
            sig.normalize_s();
            (
                sig.serialize_compact(),
                compute_mac(&ikm, &ctx, &hash).unwrap(),
            )
        };

        // Signed under the earlier tag: both checks accept it.
        let (sig, mac) = sign(PROTO_TAG);
        assert!(verify_commit(&did_key, space, author, rev, &ikm, &sig, &mac, &hash).is_ok());
        assert!(
            verify_commit_either_tag(&did_key, space, author, rev, &ikm, &sig, &mac, &hash).is_ok()
        );

        // Signed under the spec's repo tag: only the either-tag check does.
        let (sig, mac) = sign(REPO_PROTO_TAG);
        assert!(verify_commit(&did_key, space, author, rev, &ikm, &sig, &mac, &hash).is_err());
        assert!(
            verify_commit_either_tag(&did_key, space, author, rev, &ikm, &sig, &mac, &hash).is_ok()
        );

        // Neither: a tampered hash, or a commit for another space, fails.
        assert!(verify_commit_either_tag(
            &did_key, space, author, rev, &ikm, &sig, &mac, &[7u8; 32]
        )
        .is_err());
        assert!(verify_commit_either_tag(
            &did_key,
            "at://a/space/t/other",
            author,
            rev,
            &ikm,
            &sig,
            &mac,
            &hash
        )
        .is_err());
    }

    #[test]
    fn ctx_is_byte_exact() {
        // tag(16) + [len2||"sp"] + [len2||"au"] + [len2||"r"] + [len2||0xAA]
        let ctx = build_ctx("sp", "au", "r", &[0xAA]);
        let mut expected = Vec::new();
        expected.extend_from_slice(b"atproto-space-v1");
        expected.extend_from_slice(&[0x00, 0x02]);
        expected.extend_from_slice(b"sp");
        expected.extend_from_slice(&[0x00, 0x02]);
        expected.extend_from_slice(b"au");
        expected.extend_from_slice(&[0x00, 0x01]);
        expected.extend_from_slice(b"r");
        expected.extend_from_slice(&[0x00, 0x01]);
        expected.extend_from_slice(&[0xAA]);
        assert_eq!(ctx, expected);
    }

    #[test]
    fn mac_roundtrip_accepts_and_rejects() {
        let ikm = [7u8; 32];
        let ctx = build_ctx("at://a/space/t/main", "did:plc:author", "3krev", &ikm);
        let hash = [42u8; 32];
        let mac = compute_mac(&ikm, &ctx, &hash).unwrap();
        assert!(verify_mac(&ikm, &ctx, &hash, &mac).is_ok());

        // A different hash under the same context must fail.
        let other = [43u8; 32];
        assert!(matches!(
            verify_mac(&ikm, &ctx, &other, &mac),
            Err(SpaceError::BadMac)
        ));
    }

    #[test]
    fn mac_matches_cross_implementation_vector() {
        // Pinned expand-only HKDF vector, independently derived; catches an
        // accidental extract step that a roundtrip test cannot.
        let ikm = [7u8; 32];
        let ctx = build_ctx("at://a/space/t/main", "did:plc:author", "3krev", &ikm);
        let mac = compute_mac(&ikm, &ctx, &[42u8; 32]).unwrap();
        assert_eq!(
            hex::encode(mac),
            "d1bd9b47b2e0ec67c390ce4ad16460b2bf37bdd3bec33f8119e19b4001cd64b2"
        );
    }

    #[test]
    fn mac_is_ikm_bound() {
        let ctx = build_ctx("s", "a", "r", &[1u8; 32]);
        let hash = [9u8; 32];
        let mac = compute_mac(&[1u8; 32], &ctx, &hash).unwrap();
        // A different ikm keying material yields a different MAC.
        let mac2 = compute_mac(&[2u8; 32], &ctx, &hash).unwrap();
        assert_ne!(mac, mac2);
    }
}
