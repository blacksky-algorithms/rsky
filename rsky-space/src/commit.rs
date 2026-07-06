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
use sha2::Sha256;
use subtle::ConstantTimeEq;

use crate::error::{Result, SpaceError};

/// Fixed protocol tag; also names the commit format version (`ver = 1`).
const PROTO_TAG: &[u8] = b"atproto-space-v1";

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
    let mut ctx = Vec::with_capacity(
        PROTO_TAG.len() + 8 + space.len() + author.len() + rev.len() + ikm.len(),
    );
    ctx.extend_from_slice(PROTO_TAG);
    for field in [space.as_bytes(), author.as_bytes(), rev.as_bytes(), ikm] {
        ctx.extend_from_slice(&(field.len() as u16).to_be_bytes());
        ctx.extend_from_slice(field);
    }
    ctx
}

/// Compute `mac = HMAC-SHA256(HKDF-SHA256(ikm, ctx), hash)`.
pub fn compute_mac(ikm: &[u8], ctx: &[u8], hash: &[u8]) -> Result<[u8; 32]> {
    let hk = Hkdf::<Sha256>::new(None, ikm);
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
    let ctx = build_ctx(space, author, rev, ikm);
    let ok = rsky_crypto::verify::verify_signature(&did_key.to_string(), &ctx, sig, None)
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
    fn mac_is_ikm_bound() {
        let ctx = build_ctx("s", "a", "r", &[1u8; 32]);
        let hash = [9u8; 32];
        let mac = compute_mac(&[1u8; 32], &ctx, &hash).unwrap();
        // A different ikm keying material yields a different MAC.
        let mac2 = compute_mac(&[2u8; 32], &ctx, &hash).unwrap();
        assert_ne!(mac, mac2);
    }
}
