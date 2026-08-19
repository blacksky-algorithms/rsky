//! Minting signed commits for a hosted repo (spec §Commit signature).
//!
//! `ikm` is fresh per reader, so a commit is produced at serve time from the
//! repo's persisted `(rev, state)` rather than stored alongside them.
//!
//! The spec has the account's own signing key sign the commit context. A host
//! holding repos on behalf of accounts whose PDS does not implement
//! permissioned data has no access to those keys, so [`CommitSigner`] names the
//! signer explicitly instead of assuming it.

use rsky_lexicon::com::atproto::space::SignedCommit;
use rsky_space::commit::{build_ctx, compute_mac};

use crate::error::{HostError, Result};
use crate::signing::Signer;

pub const COMMIT_VERSION: i64 = 1;
pub const IKM_BYTES: usize = 32;

pub trait CommitSigner: Send + Sync {
    /// The `did:key` a reader verifies commits against.
    fn did_key(&self) -> &str;
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>>;
}

impl CommitSigner for Signer {
    fn did_key(&self) -> &str {
        Signer::did_key(self)
    }

    fn sign(&self, message: &[u8]) -> Result<Vec<u8>> {
        Signer::sign(self, message).map_err(HostError::Key)
    }
}

/// Build a signed commit over a repo's current digest.
pub fn mint_commit(
    signer: &dyn CommitSigner,
    space_uri: &str,
    author_did: &str,
    rev: &str,
    hash: &[u8; 32],
    ikm: [u8; IKM_BYTES],
) -> Result<SignedCommit> {
    let ctx = build_ctx(space_uri, author_did, rev, &ikm);
    let sig = signer.sign(&ctx)?;
    let mac = compute_mac(&ikm, &ctx, hash)?;
    Ok(SignedCommit {
        ver: COMMIT_VERSION,
        hash: hash.to_vec(),
        ikm: ikm.to_vec(),
        sig,
        mac: mac.to_vec(),
        rev: rev.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signing::test_signer;
    use rsky_space::commit::verify_commit;

    const SPACE: &str = "at://did:plc:auth/space/community.blacksky.feed/main";
    const AUTHOR: &str = "did:plc:member";

    fn minted(hash: [u8; 32], ikm: [u8; 32]) -> (Signer, SignedCommit) {
        let signer = test_signer();
        let commit = mint_commit(&signer, SPACE, AUTHOR, "3rev1", &hash, ikm).unwrap();
        (signer, commit)
    }

    fn verify(signer: &Signer, commit: &SignedCommit, hash: &[u8]) -> rsky_space::Result<()> {
        verify_commit(
            CommitSigner::did_key(signer),
            SPACE,
            AUTHOR,
            &commit.rev,
            &commit.ikm,
            &commit.sig,
            &commit.mac,
            hash,
        )
    }

    #[test]
    fn a_minted_commit_verifies() {
        let hash = [7u8; 32];
        let (signer, commit) = minted(hash, [3u8; 32]);
        assert_eq!(commit.ver, COMMIT_VERSION);
        assert_eq!(commit.hash, hash.to_vec());
        verify(&signer, &commit, &hash).unwrap();
    }

    #[test]
    fn a_tampered_hash_fails_the_mac() {
        let (signer, commit) = minted([7u8; 32], [3u8; 32]);
        assert!(verify(&signer, &commit, &[8u8; 32]).is_err());
    }

    #[test]
    fn a_different_ikm_yields_a_different_signature() {
        let hash = [7u8; 32];
        let (_, a) = minted(hash, [3u8; 32]);
        let (signer, b) = minted(hash, [4u8; 32]);
        assert_ne!(a.sig, b.sig);
        assert_ne!(a.mac, b.mac);
        verify(&signer, &b, &hash).unwrap();
    }

    #[test]
    fn a_commit_does_not_verify_under_another_space_or_author() {
        let hash = [7u8; 32];
        let (signer, commit) = minted(hash, [3u8; 32]);
        assert!(verify_commit(
            CommitSigner::did_key(&signer),
            "at://did:plc:auth/space/community.blacksky.feed/other",
            AUTHOR,
            &commit.rev,
            &commit.ikm,
            &commit.sig,
            &commit.mac,
            &hash,
        )
        .is_err());
        assert!(verify_commit(
            CommitSigner::did_key(&signer),
            SPACE,
            "did:plc:someoneelse",
            &commit.rev,
            &commit.ikm,
            &commit.sig,
            &commit.mac,
            &hash,
        )
        .is_err());
    }
}
