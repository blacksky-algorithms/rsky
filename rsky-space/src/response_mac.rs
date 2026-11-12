//! HTTP Response MAC computation for ATProto Spaces.
//!
//! This module handles MACs for HTTP responses, which are distinct from the
//! commit's internal `mac` field (handled in `commit.rs`). These MACs
//! authenticate HTTP responses to remote syncers using a shared secret derived
//! via ECDH between the author's KEM key and the syncer's ephemeral key.
//!
//! There are two MAC types:
//! - **Response Commit MAC**: Used for `getLatestCommit`, `getRepo`, and `listRepoOps`
//! - **Response Record MAC**: Used for `getRecord`

use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// Domain separator for HTTP Response Commit MACs (§Response authentication).
///
/// Used for methods that return commits: `getLatestCommit`, `getRepo`, `listRepoOps`.
pub const RESPONSE_COMMIT_TAG: &[u8] = b"atproto-space-repo-v1";

/// Domain separator for HTTP Response Record MACs (§Response authentication).
///
/// Used for methods that return individual records: `getRecord`.
pub const RESPONSE_RECORD_TAG: &[u8] = b"atproto-space-record-v1";

/// Build context for HTTP Response Commit MAC.
///
/// ```text
/// context = "atproto-space-repo-v1"
///        || uint16be(len(space)) || space
///        || uint16be(len(repo))  || repo
///        || uint16be(len(rev))   || rev
/// ```
///
/// The `data` field (32-byte SHA-256 of LtHash state) is appended separately
/// when computing the MAC, not included in the context.
pub fn build_response_commit_ctx(space: &str, repo: &str, rev: &str) -> Vec<u8> {
    let mut ctx =
        Vec::with_capacity(RESPONSE_COMMIT_TAG.len() + 6 + space.len() + repo.len() + rev.len());
    ctx.extend_from_slice(RESPONSE_COMMIT_TAG);
    for field in [space.as_bytes(), repo.as_bytes(), rev.as_bytes()] {
        ctx.extend_from_slice(&(field.len() as u16).to_be_bytes());
        ctx.extend_from_slice(field);
    }
    ctx
}

/// Compute HTTP Response Commit MAC.
///
/// `data` is the 32-byte SHA-256 of the LtHash state (the commit's `hash` field).
///
/// For `getRepo`: This MAC authenticates the commit within the STAR archive,
/// not the entire archive. The syncer extracts the commit and verifies against
/// this MAC.
///
/// ```text
/// mac = HMAC-SHA256(shared_secret, context || data)
/// ```
pub fn compute_response_commit_mac(
    shared_secret: &[u8; 32],
    space: &str,
    repo: &str,
    rev: &str,
    data: &[u8; 32],
) -> [u8; 32] {
    let ctx = build_response_commit_ctx(space, repo, rev);
    let mut input = ctx;
    input.extend_from_slice(data);

    let mut mac =
        HmacSha256::new_from_slice(shared_secret).expect("HMAC can take key of any size");
    mac.update(&input);
    mac.finalize().into_bytes().into()
}

/// Build context for HTTP Response Record MAC.
///
/// ```text
/// context = "atproto-space-record-v1"
///        || uint16be(len(uri)) || uri
/// ```
///
/// The `cid_bytes` (raw binary CID) is appended separately when computing the MAC.
pub fn build_response_record_ctx(uri: &str) -> Vec<u8> {
    let mut ctx = Vec::with_capacity(RESPONSE_RECORD_TAG.len() + 2 + uri.len());
    ctx.extend_from_slice(RESPONSE_RECORD_TAG);
    ctx.extend_from_slice(&(uri.len() as u16).to_be_bytes());
    ctx.extend_from_slice(uri.as_bytes());
    ctx
}

/// Compute HTTP Response Record MAC.
///
/// `cid_bytes` is the raw binary CID (version || codec || multihash),
/// NOT the string representation. Use `Cid::to_bytes()` from the `cid` crate.
///
/// ```text
/// mac = HMAC-SHA256(shared_secret, context || cid_bytes)
/// ```
pub fn compute_response_record_mac(
    shared_secret: &[u8; 32],
    uri: &str,
    cid_bytes: &[u8],
) -> [u8; 32] {
    let ctx = build_response_record_ctx(uri);
    let mut input = ctx;
    input.extend_from_slice(cid_bytes);

    let mut mac =
        HmacSha256::new_from_slice(shared_secret).expect("HMAC can take key of any size");
    mac.update(&input);
    mac.finalize().into_bytes().into()
}

/// Verify a MAC in constant time.
///
/// SECURITY: Always use this for MAC comparison to prevent timing attacks.
pub fn verify_response_mac(computed: &[u8; 32], received: &[u8; 32]) -> bool {
    computed.ct_eq(received).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_commit_ctx_structure() {
        let ctx = build_response_commit_ctx(
            "at://did:plc:auth/space/com.example.chat/main",
            "did:plc:author",
            "3krev123",
        );

        // Verify structure: tag || len||space || len||repo || len||rev
        let tag_len = RESPONSE_COMMIT_TAG.len();
        assert_eq!(&ctx[..tag_len], RESPONSE_COMMIT_TAG);

        // Space length (46 bytes for "at://did:plc:auth/space/com.example.chat/main")
        let space = "at://did:plc:auth/space/com.example.chat/main";
        assert_eq!(
            &ctx[tag_len..tag_len + 2],
            &(space.len() as u16).to_be_bytes()
        );
    }

    #[test]
    fn response_record_ctx_structure() {
        let uri = "at://did:plc:auth/space/t/k/did:plc:author/col/rkey";
        let ctx = build_response_record_ctx(uri);

        assert!(ctx.starts_with(RESPONSE_RECORD_TAG));
        let tag_len = RESPONSE_RECORD_TAG.len();
        assert_eq!(&ctx[tag_len..tag_len + 2], &(uri.len() as u16).to_be_bytes());
        assert_eq!(&ctx[tag_len + 2..], uri.as_bytes());
    }

    #[test]
    fn response_commit_mac_is_deterministic() {
        let secret = [0x42u8; 32];
        let data = [0xABu8; 32];

        let mac1 = compute_response_commit_mac(
            &secret,
            "at://did:plc:auth/space/t/k",
            "did:plc:author",
            "3krev",
            &data,
        );

        let mac2 = compute_response_commit_mac(
            &secret,
            "at://did:plc:auth/space/t/k",
            "did:plc:author",
            "3krev",
            &data,
        );

        assert_eq!(mac1, mac2);
        assert_eq!(mac1.len(), 32);
    }

    #[test]
    fn response_macs_differ_by_type() {
        // Same shared_secret should produce different MACs for commit vs record
        let secret = [0u8; 32];
        let commit_mac = compute_response_commit_mac(
            &secret,
            "at://space/t/k",
            "did:plc:repo",
            "rev",
            &[0u8; 32],
        );
        let record_mac = compute_response_record_mac(
            &secret,
            "at://space/t/k/did:plc:repo/col/rkey",
            &[0u8; 32],
        );
        assert_ne!(commit_mac, record_mac);
    }

    #[test]
    fn response_macs_differ_by_input() {
        let secret = [0x42u8; 32];

        // Different space produces different MAC
        let mac1 = compute_response_commit_mac(
            &secret,
            "at://space/a/k",
            "did:plc:author",
            "rev",
            &[0u8; 32],
        );
        let mac2 = compute_response_commit_mac(
            &secret,
            "at://space/b/k",
            "did:plc:author",
            "rev",
            &[0u8; 32],
        );
        assert_ne!(mac1, mac2);

        // Different data produces different MAC
        let mac3 = compute_response_commit_mac(
            &secret,
            "at://space/a/k",
            "did:plc:author",
            "rev",
            &[1u8; 32],
        );
        assert_ne!(mac1, mac3);
    }

    #[test]
    fn constant_time_verification() {
        let mac1 = [0xAAu8; 32];
        let mac2 = [0xAAu8; 32];
        let mac3 = [0xBBu8; 32];

        assert!(verify_response_mac(&mac1, &mac2));
        assert!(!verify_response_mac(&mac1, &mac3));
    }

    #[test]
    fn response_record_mac_with_cid_bytes() {
        let secret = [0x42u8; 32];
        // Simulated CID bytes (version || codec || multihash)
        let cid_bytes = [0x01, 0x71, 0x12, 0x20, 0xAA, 0xBB, 0xCC, 0xDD];

        let mac = compute_response_record_mac(
            &secret,
            "at://did:plc:auth/space/t/k/did:plc:author/col/rkey",
            &cid_bytes,
        );

        assert_eq!(mac.len(), 32);

        // Different CID produces different MAC
        let other_cid = [0x01, 0x71, 0x12, 0x20, 0xFF, 0xFF, 0xFF, 0xFF];
        let mac2 = compute_response_record_mac(
            &secret,
            "at://did:plc:auth/space/t/k/did:plc:author/col/rkey",
            &other_cid,
        );
        assert_ne!(mac, mac2);
    }

    #[test]
    fn response_commit_mac_matches_pinned_vector() {
        // Pinned test vector for cross-implementation verification
        let secret = [0x42u8; 32];
        let data = [0xABu8; 32];

        let mac = compute_response_commit_mac(
            &secret,
            "at://did:plc:test/space/com.example/main",
            "did:plc:author",
            "3k12345",
            &data,
        );

        // This vector should remain stable across implementations
        // mac = HMAC-SHA256(secret, context || data)
        // context = "atproto-space-repo-v1" || len||space || len||repo || len||rev
        assert_eq!(
            hex::encode(mac),
            "a3b1abb0c08241e0403fc6a9a2cfcf28e3dba5c8f18deadaee5c936e5e48e780"
        );
    }
}
