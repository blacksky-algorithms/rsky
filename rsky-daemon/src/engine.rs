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
    /// Ops whose `prev` did not match our indexed cid: evidence of missed
    /// operations, caught for certain by the terminal hash check.
    pub prev_mismatches: usize,
    /// True when a signed commit was present and authenticated the new head.
    pub commit_verified: bool,
    /// The head revision now indexed, if advanced.
    pub rev: Option<String>,
}

/// Incrementally sync a single author's permissioned repo into the index,
/// paging the oplog via its cursor until the host returns the terminal page
/// (the one carrying the signed commit).
pub async fn sync_repo(
    client: &dyn RepoHostClient,
    index: &dyn SpaceIndex,
    keys: &dyn CommitKeyResolver,
    space_uri: &str,
    did: &str,
) -> Result<SyncOutcome> {
    let since = index.last_rev(did).await?;
    let mut lth = index.load_lthash(did).await?;
    let mut ops_applied = 0usize;
    let mut prev_mismatches = 0usize;
    let mut cursor: Option<String> = None;
    let mut last_rev: Option<String> = None;

    loop {
        let page = client
            .list_repo_ops(space_uri, did, since.as_deref(), cursor.as_deref())
            .await?;
        for op in &page.ops {
            let old_cid = index.get_cid(did, &op.collection, &op.rkey).await?;
            if let Some(prev) = &op.prev {
                if old_cid.as_deref() != Some(prev.as_str()) {
                    // A missed operation; proceed, since correctness rests on
                    // the terminal hash check, not on receiving every op.
                    tracing::warn!(did, collection = %op.collection, rkey = %op.rkey, prev = %prev, indexed = ?old_cid, "op prev does not match indexed cid");
                    prev_mismatches += 1;
                }
            }
            // Remove the prior element for this path (update or delete supersedes it).
            if let Some(old) = &old_cid {
                lth.remove(&element(&op.collection, &op.rkey, old));
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
        if let Some(op) = page.ops.last() {
            last_rev = Some(op.rev.clone());
        }

        if let Some(commit) = &page.commit {
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
            return Ok(SyncOutcome {
                ops_applied,
                prev_mismatches,
                commit_verified: true,
                rev: Some(commit.rev.clone()),
            });
        }
        match page.cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }

    // Exhausted the oplog without a terminal commit: advance to the last op's
    // rev without a hash check; a later sync carries the commit.
    if let Some(rev) = &last_rev {
        index.save_head(did, rev, &lth).await?;
    }
    Ok(SyncOutcome {
        ops_applied,
        prev_mismatches,
        commit_verified: false,
        rev: last_rev,
    })
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

    /// Serves fixed oplog pages: cursor "n" fetches page n, page i links to
    /// page i+1 unless it is the last.
    struct StaticHost(Vec<OplogPage>);
    impl StaticHost {
        fn single(page: OplogPage) -> Self {
            Self(vec![page])
        }
    }
    #[async_trait]
    impl RepoHostClient for StaticHost {
        async fn list_repo_ops(
            &self,
            _space: &str,
            _did: &str,
            _since: Option<&str>,
            cursor: Option<&str>,
        ) -> Result<OplogPage> {
            let i: usize = cursor.map(|c| c.parse().unwrap()).unwrap_or(0);
            let page = &self.0[i];
            Ok(OplogPage {
                ops: page.ops.clone(),
                commit: page.commit.clone(),
                cursor: (i + 1 < self.0.len()).then(|| (i + 1).to_string()),
            })
        }
        async fn get_repo_car(&self, _space: &str, _did: &str) -> Result<Vec<u8>> {
            Err(DaemonError::Xrpc("no car in static host".to_string()))
        }
        async fn get_latest_commit(&self, _space: &str, _did: &str) -> Result<SignedCommit> {
            Err(DaemonError::Xrpc("no commit in static host".to_string()))
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
        let host = StaticHost::single(OplogPage {
            ops,
            commit: Some(commit),
            cursor: None,
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
        let host1 = StaticHost::single(OplogPage {
            ops: vec![op(coll, "3ka", Some("bafyA"), "3rev1")],
            commit: Some(c1),
            cursor: None,
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
        let host2 = StaticHost::single(OplogPage {
            ops: vec![op(coll, "3ka", None, "3rev2")],
            commit: Some(commit2),
            cursor: None,
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
        let host = StaticHost::single(OplogPage {
            ops: vec![op(coll, "3ka", Some("bafyA"), "3rev")],
            commit: None,
            cursor: None,
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
        let host = StaticHost::single(OplogPage {
            ops: vec![],
            commit: None,
            cursor: None,
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
        let host = StaticHost::single(OplogPage {
            ops: vec![op(coll, "3ka", Some("bafyDIFFERENT"), "3rev")],
            commit: Some(commit),
            cursor: None,
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
        let host = StaticHost::single(OplogPage {
            ops: vec![op(coll, "3ka", Some("bafyA"), "3rev")],
            commit: Some(commit),
            cursor: None,
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

    #[tokio::test]
    async fn paginates_until_terminal_commit_page() {
        let a = author();
        let coll = "community.blacksky.feed.post";
        let elements = vec![
            (coll.to_string(), "3ka".to_string(), "bafyA".to_string()),
            (coll.to_string(), "3kb".to_string(), "bafyB".to_string()),
            (coll.to_string(), "3kc".to_string(), "bafyC".to_string()),
        ];
        let commit = signed_commit(&a, &elements, "3rev3");
        let host = StaticHost(vec![
            OplogPage {
                ops: vec![op(coll, "3ka", Some("bafyA"), "3rev1")],
                commit: None,
                cursor: None,
            },
            OplogPage {
                ops: vec![op(coll, "3kb", Some("bafyB"), "3rev2")],
                commit: None,
                cursor: None,
            },
            OplogPage {
                ops: vec![op(coll, "3kc", Some("bafyC"), "3rev3")],
                commit: Some(commit),
                cursor: None,
            },
        ]);
        let index = InMemoryIndex::new();
        let keys = FixedKey(a.did_key.clone());

        let outcome = sync_repo(&host, &index, &keys, SPACE, AUTHOR)
            .await
            .unwrap();
        assert_eq!(outcome.ops_applied, 3);
        assert!(outcome.commit_verified);
        assert_eq!(outcome.rev.as_deref(), Some("3rev3"));
        assert_eq!(index.record_count(AUTHOR), 3);
    }

    async fn prev_mismatch_scenario() {
        let a = author();
        let coll = "community.blacksky.feed.post";
        // A create, an in-order update whose prev matches it, and an update
        // referencing a prev we never indexed (a missed create).
        let create = op(coll, "3kz", Some("bafyZ"), "3rev1");
        let mut ordered = op(coll, "3kz", Some("bafyZ2"), "3rev2");
        ordered.prev = Some("bafyZ".to_string());
        let mut missed = op(coll, "3ka", Some("bafyNew"), "3rev2");
        missed.prev = Some("bafyMissed".to_string());
        let commit = signed_commit(
            &a,
            &[
                (coll.to_string(), "3ka".to_string(), "bafyNew".to_string()),
                (coll.to_string(), "3kz".to_string(), "bafyZ2".to_string()),
            ],
            "3rev2",
        );
        let host = StaticHost::single(OplogPage {
            ops: vec![create, ordered, missed],
            commit: Some(commit),
            cursor: None,
        });
        let index = InMemoryIndex::new();
        let keys = FixedKey(a.did_key.clone());

        let outcome = sync_repo(&host, &index, &keys, SPACE, AUTHOR)
            .await
            .unwrap();
        assert!(outcome.commit_verified);
        assert_eq!(outcome.prev_mismatches, 1);
        assert_eq!(index.record(AUTHOR, coll, "3ka").unwrap().cid, "bafyNew");
        assert_eq!(index.record(AUTHOR, coll, "3kz").unwrap().cid, "bafyZ2");
    }

    #[tokio::test]
    async fn prev_mismatch_warns_but_hash_check_still_governs() {
        // Run once without a subscriber and once with, so both branches of
        // the warn's level gate are exercised.
        prev_mismatch_scenario().await;
        let _guard = tracing::subscriber::set_default(
            tracing_subscriber::fmt()
                .with_max_level(tracing::Level::TRACE)
                .finish(),
        );
        prev_mismatch_scenario().await;
    }

    #[tokio::test]
    async fn static_host_stubs_error() {
        let host = StaticHost::single(OplogPage {
            ops: vec![],
            commit: None,
            cursor: None,
        });
        assert!(host.get_repo_car(SPACE, AUTHOR).await.is_err());
        assert!(host.get_latest_commit(SPACE, AUTHOR).await.is_err());
    }
}
