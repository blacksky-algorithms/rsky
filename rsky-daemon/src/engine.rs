//! The sync engine: apply a repo's operation log to the index, maintain the
//! running LtHash, and authenticate the result against the signed commit.
//!
//! Correctness rests on the set-hash comparison, not on receiving every op: a
//! missed or out-of-order operation is caught when the running hash disagrees
//! with the signed commit, at which point the caller falls back to full-state
//! recovery (`getRepo`). This mirrors the proposal's self-healing sync.

use async_trait::async_trait;
use rsky_space::commit::verify_commit;
use rsky_space::lthash::element;

use crate::error::{DaemonError, Result};
use crate::index::SpaceIndex;
use crate::repohost::RepoHostClient;

/// Resolves an author's atproto signing `did:key` to verify their commit.
#[async_trait]
pub trait CommitKeyResolver: Send + Sync {
    async fn signing_key(&self, did: &str) -> Result<String>;
}

/// Result of syncing one repo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncOutcome {
    pub ops_applied: usize,
    /// True when a signed commit was present and authenticated the new head.
    pub commit_verified: bool,
    /// The head revision now indexed, if advanced.
    pub rev: Option<String>,
}

/// Incrementally sync a single author's permissioned repo into the index.
pub async fn sync_repo(
    client: &dyn RepoHostClient,
    index: &dyn SpaceIndex,
    keys: &dyn CommitKeyResolver,
    space_uri: &str,
    did: &str,
) -> Result<SyncOutcome> {
    let since = index.last_rev(did).await?;
    let page = client.list_repo_ops(did, since.as_deref()).await?;

    let mut lth = index.load_lthash(did).await?;
    let mut ops_applied = 0usize;

    for op in &page.ops {
        // Remove the prior element for this path (update or delete supersedes it).
        if let Some(old_cid) = index.get_cid(did, &op.collection, &op.rkey).await? {
            lth.remove(&element(&op.collection, &op.rkey, &old_cid));
        }
        match &op.cid {
            Some(cid) => {
                index
                    .upsert(
                        did,
                        &op.collection,
                        &op.rkey,
                        cid,
                        &op.rev,
                        op.value.as_ref().map(|v| v.to_vec()),
                    )
                    .await?;
                lth.add(&element(&op.collection, &op.rkey, cid));
            }
            None => {
                index.delete(did, &op.collection, &op.rkey).await?;
            }
        }
        ops_applied += 1;
    }

    match &page.commit {
        Some(commit) => {
            let author_key = keys.signing_key(did).await?;
            // Authenticate the commit: signature over ctx + MAC binding its hash.
            verify_commit(
                &author_key,
                space_uri,
                did,
                &commit.rev,
                &commit.ikm,
                &commit.sig,
                &commit.mac,
                &commit.hash,
            )?;
            // Consistency: our independently-derived hash must equal the commit's.
            if lth.hash().as_slice() != commit.hash.as_slice() {
                return Err(DaemonError::Diverged(did.to_string()));
            }
            index.save_head(did, &commit.rev, &lth).await?;
            Ok(SyncOutcome {
                ops_applied,
                commit_verified: true,
                rev: Some(commit.rev.clone()),
            })
        }
        None => {
            // No terminal commit in this page: advance to the last op's rev
            // without a hash check; a later page carries the commit.
            let rev = page.ops.last().map(|o| o.rev.clone());
            if let Some(r) = &rev {
                index.save_head(did, r, &lth).await?;
            }
            Ok(SyncOutcome {
                ops_applied,
                commit_verified: false,
                rev,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::InMemoryIndex;
    use crate::repohost::OplogPage;
    use rsky_space::commit::{build_ctx, compute_mac};
    use rsky_space::types::{RepoOp, SignedCommit};
    use secp256k1::{Message, PublicKey, Secp256k1, SecretKey};
    use serde_bytes::ByteBuf;
    use sha2::{Digest, Sha256};

    const SPACE: &str = "at://did:plc:authority/space/community.blacksky.feed/main";
    const AUTHOR: &str = "did:plc:author";

    struct Author {
        secret: SecretKey,
        did_key: String,
    }
    fn author() -> Author {
        let secret = SecretKey::from_slice(&[0x22u8; 32]).unwrap();
        let secp = Secp256k1::new();
        let pubkey = PublicKey::from_secret_key(&secp, &secret);
        Author {
            secret,
            did_key: rsky_crypto::utils::encode_did_key(&pubkey),
        }
    }
    fn sign_ctx(secret: &SecretKey, ctx: &[u8]) -> Vec<u8> {
        let hash = Sha256::digest(ctx);
        let msg = Message::from_digest_slice(hash.as_ref()).unwrap();
        let mut sig = secret.sign_ecdsa(msg);
        sig.normalize_s();
        sig.serialize_compact().to_vec()
    }

    struct FixedKey(String);
    #[async_trait]
    impl CommitKeyResolver for FixedKey {
        async fn signing_key(&self, _did: &str) -> Result<String> {
            Ok(self.0.clone())
        }
    }

    struct StaticHost(OplogPage);
    #[async_trait]
    impl RepoHostClient for StaticHost {
        async fn list_repo_ops(&self, _did: &str, _since: Option<&str>) -> Result<OplogPage> {
            Ok(OplogPage {
                ops: self.0.ops.clone(),
                commit: self.0.commit.clone(),
            })
        }
    }

    fn op(collection: &str, rkey: &str, cid: Option<&str>, rev: &str) -> RepoOp {
        RepoOp {
            rev: rev.to_string(),
            collection: collection.to_string(),
            rkey: rkey.to_string(),
            cid: cid.map(|c| c.to_string()),
            prev: None,
            value: cid.map(|_| ByteBuf::from(vec![1u8, 2, 3])),
        }
    }

    /// Build a valid signed commit over the given elements at `rev`.
    fn signed_commit(a: &Author, elements: &[(String, String, String)], rev: &str) -> SignedCommit {
        let mut lth = rsky_space::LtHash::new();
        for (c, r, cid) in elements {
            lth.add(&element(c, r, cid));
        }
        let hash = lth.hash();
        let ikm = [9u8; 32];
        let ctx = build_ctx(SPACE, AUTHOR, rev, &ikm);
        let sig = sign_ctx(&a.secret, &ctx);
        let mac = compute_mac(&ikm, &ctx, &hash).unwrap();
        SignedCommit {
            ver: 1,
            hash: ByteBuf::from(hash.to_vec()),
            ikm: ByteBuf::from(ikm.to_vec()),
            sig: ByteBuf::from(sig),
            mac: ByteBuf::from(mac.to_vec()),
            rev: rev.to_string(),
        }
    }

    #[tokio::test]
    async fn applies_ops_and_verifies_commit() {
        let a = author();
        let coll = "community.blacksky.feed.post";
        let ops = vec![
            op(coll, "3ka", Some("bafyA"), "3rev"),
            op(coll, "3kb", Some("bafyB"), "3rev"),
        ];
        let elements = vec![
            (coll.to_string(), "3ka".to_string(), "bafyA".to_string()),
            (coll.to_string(), "3kb".to_string(), "bafyB".to_string()),
        ];
        let commit = signed_commit(&a, &elements, "3rev");
        let host = StaticHost(OplogPage {
            ops,
            commit: Some(commit),
        });
        let index = InMemoryIndex::new();
        let keys = FixedKey(a.did_key.clone());

        let outcome = sync_repo(&host, &index, &keys, SPACE, AUTHOR)
            .await
            .unwrap();
        assert_eq!(outcome.ops_applied, 2);
        assert!(outcome.commit_verified);
        assert_eq!(outcome.rev.as_deref(), Some("3rev"));
        assert_eq!(index.record_count(AUTHOR), 2);
        assert_eq!(index.record(AUTHOR, coll, "3ka").unwrap().cid, "bafyA");
        assert_eq!(
            index.last_rev(AUTHOR).await.unwrap().as_deref(),
            Some("3rev")
        );
    }

    #[tokio::test]
    async fn delete_removes_record_and_element() {
        let a = author();
        let coll = "community.blacksky.feed.post";
        // First sync: one create.
        let index = InMemoryIndex::new();
        let keys = FixedKey(a.did_key.clone());
        let c1 = signed_commit(
            &a,
            &[(coll.to_string(), "3ka".to_string(), "bafyA".to_string())],
            "3rev1",
        );
        let host1 = StaticHost(OplogPage {
            ops: vec![op(coll, "3ka", Some("bafyA"), "3rev1")],
            commit: Some(c1),
        });
        sync_repo(&host1, &index, &keys, SPACE, AUTHOR)
            .await
            .unwrap();
        assert_eq!(index.record_count(AUTHOR), 1);

        // Second sync: delete it -> empty repo -> all-zero LtHash.
        let empty = rsky_space::LtHash::new();
        let ikm = [9u8; 32];
        let ctx = build_ctx(SPACE, AUTHOR, "3rev2", &ikm);
        let commit2 = SignedCommit {
            ver: 1,
            hash: ByteBuf::from(empty.hash().to_vec()),
            ikm: ByteBuf::from(ikm.to_vec()),
            sig: ByteBuf::from(sign_ctx(&a.secret, &ctx)),
            mac: ByteBuf::from(compute_mac(&ikm, &ctx, &empty.hash()).unwrap().to_vec()),
            rev: "3rev2".to_string(),
        };
        let host2 = StaticHost(OplogPage {
            ops: vec![op(coll, "3ka", None, "3rev2")],
            commit: Some(commit2),
        });
        let outcome = sync_repo(&host2, &index, &keys, SPACE, AUTHOR)
            .await
            .unwrap();
        assert!(outcome.commit_verified);
        assert_eq!(index.record_count(AUTHOR), 0);
    }

    #[tokio::test]
    async fn page_without_commit_advances_unverified() {
        let coll = "community.blacksky.feed.post";
        let host = StaticHost(OplogPage {
            ops: vec![op(coll, "3ka", Some("bafyA"), "3rev")],
            commit: None,
        });
        let index = InMemoryIndex::new();
        let keys = FixedKey(author().did_key);
        let outcome = sync_repo(&host, &index, &keys, SPACE, AUTHOR)
            .await
            .unwrap();
        assert_eq!(outcome.ops_applied, 1);
        assert!(!outcome.commit_verified);
        assert_eq!(outcome.rev.as_deref(), Some("3rev"));
        assert_eq!(
            index.last_rev(AUTHOR).await.unwrap().as_deref(),
            Some("3rev")
        );
    }

    #[tokio::test]
    async fn empty_page_without_commit_is_a_noop() {
        let host = StaticHost(OplogPage {
            ops: vec![],
            commit: None,
        });
        let index = InMemoryIndex::new();
        let keys = FixedKey(author().did_key);
        let outcome = sync_repo(&host, &index, &keys, SPACE, AUTHOR)
            .await
            .unwrap();
        assert_eq!(outcome.ops_applied, 0);
        assert_eq!(outcome.rev, None);
        assert_eq!(index.last_rev(AUTHOR).await.unwrap(), None);
    }

    #[tokio::test]
    async fn divergent_hash_is_rejected() {
        let a = author();
        let coll = "community.blacksky.feed.post";
        // Commit signed over one element, but the op stream carries a different cid,
        // so our derived hash won't match the committed hash.
        let commit = signed_commit(
            &a,
            &[(coll.to_string(), "3ka".to_string(), "bafyA".to_string())],
            "3rev",
        );
        let host = StaticHost(OplogPage {
            ops: vec![op(coll, "3ka", Some("bafyDIFFERENT"), "3rev")],
            commit: Some(commit),
        });
        let index = InMemoryIndex::new();
        let keys = FixedKey(a.did_key.clone());
        let res = sync_repo(&host, &index, &keys, SPACE, AUTHOR).await;
        assert!(matches!(res, Err(DaemonError::Diverged(_))));
    }

    #[tokio::test]
    async fn bad_signature_is_rejected() {
        let a = author();
        let wrong = author_wrong();
        let coll = "community.blacksky.feed.post";
        let commit = signed_commit(
            &a,
            &[(coll.to_string(), "3ka".to_string(), "bafyA".to_string())],
            "3rev",
        );
        let host = StaticHost(OplogPage {
            ops: vec![op(coll, "3ka", Some("bafyA"), "3rev")],
            commit: Some(commit),
        });
        let index = InMemoryIndex::new();
        // Resolve to the WRONG author key -> signature check fails.
        let keys = FixedKey(wrong.did_key.clone());
        let res = sync_repo(&host, &index, &keys, SPACE, AUTHOR).await;
        assert!(matches!(
            res,
            Err(DaemonError::Space(rsky_space::SpaceError::BadSignature))
        ));
    }

    fn author_wrong() -> Author {
        let secret = SecretKey::from_slice(&[0x33u8; 32]).unwrap();
        let secp = Secp256k1::new();
        let pubkey = PublicKey::from_secret_key(&secp, &secret);
        Author {
            secret,
            did_key: rsky_crypto::utils::encode_did_key(&pubkey),
        }
    }
}
