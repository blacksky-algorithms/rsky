//! Full-state recovery (proposal §Full-state recovery): when incremental sync
//! cannot proceed (diverged hash, oplog window exceeded), fetch the whole repo
//! as a CAR, validate it end-to-end, and diff it against the local index.

use std::collections::HashSet;

use rsky_space::car::RepoCarValidator;
use rsky_space::commit::verify_commit;
use rsky_space::lthash::{element, LtHash};

use crate::engine::{CommitKeyResolver, SyncOutcome};
use crate::error::{DaemonError, Result};
use crate::index::SpaceIndex;
use crate::repohost::RepoHostClient;

/// Recover an author's repo from a full-state CAR: verify the commit with the
/// author's resolved key, authenticate the index against the trusted hash,
/// upsert missing/changed records, delete records absent from the CAR, rebuild
/// the LtHash from the validated entries, and save the new head.
pub async fn recover_repo(
    client: &dyn RepoHostClient,
    index: &dyn SpaceIndex,
    keys: &dyn CommitKeyResolver,
    space_uri: &str,
    did: &str,
) -> Result<SyncOutcome> {
    let car = client.get_repo_car(space_uri, did).await?;
    let validator = RepoCarValidator::new(car.as_slice()).await?;
    let commit = validator.commit().clone();
    let author_key = keys.signing_key(did).await?;
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
    let records = validator.into_records(&commit.hash).await?;

    let mut lth = LtHash::new();
    let mut keep: HashSet<String> = HashSet::with_capacity(records.len());
    let mut changed = 0usize;
    for (path, cid, bytes) in &records {
        let (collection, rkey) = path
            .split_once('/')
            .ok_or_else(|| DaemonError::Index(format!("malformed CAR index path: {path}")))?;
        let cid = cid.to_string();
        if index.get_cid(did, collection, rkey).await?.as_deref() != Some(cid.as_str()) {
            index
                .upsert(
                    did,
                    collection,
                    rkey,
                    &cid,
                    &commit.rev,
                    Some(bytes.clone()),
                )
                .await?;
            changed += 1;
        }
        lth.add(&element(collection, rkey, &cid));
        keep.insert(path.clone());
    }
    for (collection, rkey, _) in index.list_paths(did).await? {
        if !keep.contains(&format!("{collection}/{rkey}")) {
            index.delete(did, &collection, &rkey).await?;
            changed += 1;
        }
    }
    index.save_head(did, &commit.rev, &lth).await?;
    tracing::info!(did, rev = %commit.rev, changed, "recovered repo from full-state CAR");
    Ok(SyncOutcome {
        ops_applied: changed,
        prev_mismatches: 0,
        commit_verified: true,
        rev: Some(commit.rev),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::index::InMemoryIndex;
    use crate::repohost::OplogPage;
    use async_trait::async_trait;
    use lexicon_cid::multihash::Multihash;
    use lexicon_cid::Cid;
    use rsky_space::car::repo_car_bytes;
    use rsky_space::commit::{build_ctx, compute_mac};
    use rsky_space::types::SignedCommit;
    use secp256k1::{Message, PublicKey, Secp256k1, SecretKey};
    use serde_bytes::ByteBuf;
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;

    pub(crate) const SPACE: &str = "at://did:plc:authority/space/community.blacksky.feed/main";
    pub(crate) const AUTHOR: &str = "did:plc:author";
    const REV: &str = "3rev";
    const IKM: [u8; 32] = [9u8; 32];
    const RAW: u64 = 0x55;

    pub(crate) struct Author {
        pub secret: SecretKey,
        pub did_key: String,
    }

    pub(crate) fn author() -> Author {
        let secret = SecretKey::from_slice(&[0x22u8; 32]).unwrap();
        let secp = Secp256k1::new();
        let pubkey = PublicKey::from_secret_key(&secp, &secret);
        Author {
            secret,
            did_key: rsky_crypto::utils::encode_did_key(&pubkey),
        }
    }

    pub(crate) fn raw_block(text: &str) -> (Cid, Vec<u8>) {
        let bytes = text.as_bytes().to_vec();
        let digest = Sha256::digest(&bytes);
        let multihash = Multihash::wrap(0x12, &digest).unwrap();
        (Cid::new_v1(RAW, multihash), bytes)
    }

    pub(crate) fn signed_commit_for(
        author: &Author,
        entries: &BTreeMap<String, Cid>,
        rev: &str,
    ) -> SignedCommit {
        let mut lth = LtHash::new();
        for (path, cid) in entries {
            let (collection, rkey) = path.split_once('/').unwrap();
            lth.add(&element(collection, rkey, &cid.to_string()));
        }
        let hash = lth.hash();
        let ctx = build_ctx(SPACE, AUTHOR, rev, &IKM);
        let digest = Sha256::digest(&ctx);
        let msg = Message::from_digest_slice(&digest).unwrap();
        let mut sig = author.secret.sign_ecdsa(msg);
        sig.normalize_s();
        let mac = compute_mac(&IKM, &ctx, &hash).unwrap();
        SignedCommit {
            ver: 1,
            hash: ByteBuf::from(hash.to_vec()),
            ikm: ByteBuf::from(IKM.to_vec()),
            sig: ByteBuf::from(sig.serialize_compact().to_vec()),
            mac: ByteBuf::from(mac.to_vec()),
            rev: rev.to_string(),
        }
    }

    pub(crate) struct Fixture {
        pub author: Author,
        pub entries: BTreeMap<String, Cid>,
        pub store: BTreeMap<Cid, Vec<u8>>,
    }

    pub(crate) fn fixture() -> Fixture {
        let author = author();
        let mut entries = BTreeMap::new();
        let mut store = BTreeMap::new();
        for (path, text) in [
            ("community.blacksky.feed.like/3kc", "like one"),
            ("community.blacksky.feed.post/3ka", "post one"),
            ("community.blacksky.feed.post/3kb", "post two"),
        ] {
            let (cid, bytes) = raw_block(text);
            entries.insert(path.to_string(), cid);
            store.insert(cid, bytes);
        }
        Fixture {
            author,
            entries,
            store,
        }
    }

    pub(crate) async fn car_bytes(f: &Fixture, commit: &SignedCommit) -> Vec<u8> {
        let store = f.store.clone();
        repo_car_bytes(commit, &f.entries, move |cid| store.get(cid).cloned())
            .await
            .unwrap()
    }

    /// Serves a fixed CAR for `get_repo_car`; oplog and latest-commit unused.
    pub(crate) struct CarHost(pub Vec<u8>);
    #[async_trait]
    impl RepoHostClient for CarHost {
        async fn list_repo_ops(
            &self,
            _space: &str,
            _did: &str,
            _since: Option<&str>,
            _cursor: Option<&str>,
        ) -> Result<OplogPage> {
            Err(DaemonError::Xrpc("no oplog in car host".to_string()))
        }
        async fn get_repo_car(&self, _space: &str, _did: &str) -> Result<Vec<u8>> {
            Ok(self.0.clone())
        }
        async fn get_latest_commit(&self, _space: &str, _did: &str) -> Result<SignedCommit> {
            Err(DaemonError::Xrpc("no commit in car host".to_string()))
        }
    }

    pub(crate) struct FixedKey(pub String);
    #[async_trait]
    impl CommitKeyResolver for FixedKey {
        async fn signing_key(&self, _did: &str) -> Result<String> {
            Ok(self.0.clone())
        }
    }

    #[tokio::test]
    async fn recovers_into_empty_index() {
        let f = fixture();
        let commit = signed_commit_for(&f.author, &f.entries, REV);
        let host = CarHost(car_bytes(&f, &commit).await);
        let index = InMemoryIndex::new();
        let keys = FixedKey(f.author.did_key.clone());

        let outcome = recover_repo(&host, &index, &keys, SPACE, AUTHOR)
            .await
            .unwrap();
        assert!(outcome.commit_verified);
        assert_eq!(outcome.ops_applied, 3);
        assert_eq!(outcome.rev.as_deref(), Some(REV));
        assert_eq!(index.record_count(AUTHOR), 3);
        let stored = index
            .record(AUTHOR, "community.blacksky.feed.post", "3ka")
            .unwrap();
        assert_eq!(stored.value.as_deref(), Some(b"post one".as_slice()));
        assert_eq!(index.last_rev(AUTHOR).await.unwrap().as_deref(), Some(REV));

        // Recovered head hash equals the commit hash: a subsequent sweep sees
        // this repo as current.
        let lth = index.load_lthash(AUTHOR).await.unwrap();
        assert_eq!(lth.hash().to_vec(), commit.hash.to_vec());

        // CarHost's unused trait methods, for completeness.
        assert!(host.list_repo_ops(SPACE, AUTHOR, None, None).await.is_err());
        assert!(host.get_latest_commit(SPACE, AUTHOR).await.is_err());
    }

    #[tokio::test]
    async fn recovery_replaces_stale_and_deletes_extra_records() {
        let f = fixture();
        let commit = signed_commit_for(&f.author, &f.entries, REV);
        let host = CarHost(car_bytes(&f, &commit).await);
        let index = InMemoryIndex::new();
        let keys = FixedKey(f.author.did_key.clone());

        // Diverged copy: one stale cid for a path in the CAR, one record the
        // CAR no longer contains, one already-correct record.
        index
            .upsert(
                AUTHOR,
                "community.blacksky.feed.post",
                "3ka",
                "bafyStale",
                "3old",
                Some(b"stale".to_vec()),
            )
            .await
            .unwrap();
        index
            .upsert(
                AUTHOR,
                "community.blacksky.feed.post",
                "3gone",
                "bafyGone",
                "3old",
                None,
            )
            .await
            .unwrap();
        let like_cid = f.entries["community.blacksky.feed.like/3kc"].to_string();
        index
            .upsert(
                AUTHOR,
                "community.blacksky.feed.like",
                "3kc",
                &like_cid,
                "3old",
                Some(b"like one".to_vec()),
            )
            .await
            .unwrap();

        let outcome = recover_repo(&host, &index, &keys, SPACE, AUTHOR)
            .await
            .unwrap();
        // Changed: 2 upserts (stale replaced + missing added) + 1 delete.
        assert_eq!(outcome.ops_applied, 3);
        assert_eq!(index.record_count(AUTHOR), 3);
        assert_eq!(
            index
                .record(AUTHOR, "community.blacksky.feed.post", "3ka")
                .unwrap()
                .value
                .as_deref(),
            Some(b"post one".as_slice())
        );
        assert!(index
            .record(AUTHOR, "community.blacksky.feed.post", "3gone")
            .is_none());
    }

    #[tokio::test]
    async fn commit_signed_by_wrong_key_is_rejected() {
        let f = fixture();
        let wrong = Author {
            secret: SecretKey::from_slice(&[0x33u8; 32]).unwrap(),
            did_key: String::new(),
        };
        let forged = Author {
            secret: wrong.secret,
            did_key: f.author.did_key.clone(),
        };
        let commit = signed_commit_for(&forged, &f.entries, REV);
        let host = CarHost(car_bytes(&f, &commit).await);
        let index = InMemoryIndex::new();
        let keys = FixedKey(f.author.did_key.clone());

        let err = recover_repo(&host, &index, &keys, SPACE, AUTHOR)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            DaemonError::Space(rsky_space::SpaceError::BadSignature)
        ));
        assert_eq!(index.record_count(AUTHOR), 0);
    }

    #[tokio::test]
    async fn tampered_car_index_is_rejected() {
        let f = fixture();
        // Commit over a subset: the CAR's index no longer authenticates.
        let mut subset = f.entries.clone();
        subset.remove("community.blacksky.feed.like/3kc");
        let commit = signed_commit_for(&f.author, &subset, REV);
        let host = CarHost(car_bytes(&f, &commit).await);
        let index = InMemoryIndex::new();
        let keys = FixedKey(f.author.did_key.clone());

        let err = recover_repo(&host, &index, &keys, SPACE, AUTHOR)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            DaemonError::Space(rsky_space::SpaceError::IndexHashMismatch)
        ));
        assert_eq!(index.record_count(AUTHOR), 0);
    }

    #[tokio::test]
    async fn malformed_index_path_is_rejected() {
        let author = author();
        let (cid, bytes) = raw_block("record");
        let mut entries = BTreeMap::new();
        entries.insert("nopath".to_string(), cid);
        // A commit whose hash covers the malformed path, so the failure is
        // ours (path parsing), not the CAR validator's.
        let mut lth = LtHash::new();
        lth.add(&format!("nopath/{cid}"));
        let hash = lth.hash();
        let ctx = build_ctx(SPACE, AUTHOR, REV, &IKM);
        let digest = Sha256::digest(&ctx);
        let msg = Message::from_digest_slice(&digest).unwrap();
        let mut sig = author.secret.sign_ecdsa(msg);
        sig.normalize_s();
        let commit = SignedCommit {
            ver: 1,
            hash: ByteBuf::from(hash.to_vec()),
            ikm: ByteBuf::from(IKM.to_vec()),
            sig: ByteBuf::from(sig.serialize_compact().to_vec()),
            mac: ByteBuf::from(compute_mac(&IKM, &ctx, &hash).unwrap().to_vec()),
            rev: REV.to_string(),
        };
        let car = repo_car_bytes(&commit, &entries, move |c| {
            (*c == cid).then(|| bytes.clone())
        })
        .await
        .unwrap();

        let host = CarHost(car);
        let index = InMemoryIndex::new();
        let keys = FixedKey(author.did_key.clone());
        let err = recover_repo(&host, &index, &keys, SPACE, AUTHOR)
            .await
            .unwrap_err();
        assert!(matches!(err, DaemonError::Index(m) if m.contains("nopath")));
    }

    #[tokio::test]
    async fn garbage_car_is_rejected() {
        let host = CarHost(vec![0xFFu8; 16]);
        let index = InMemoryIndex::new();
        let keys = FixedKey(author().did_key);
        let err = recover_repo(&host, &index, &keys, SPACE, AUTHOR)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            DaemonError::Space(rsky_space::SpaceError::Car(_))
        ));
    }
}
