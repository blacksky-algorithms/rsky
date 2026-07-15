//! Permissioned repo CAR serialization and streaming validation
//! (proposal §Repo serialization, §Full-state recovery).
//!
//! A permissioned repo CAR (CARv1) declares two roots, in order: the signed
//! commit block, then an index block mapping `"{collection}/{rkey}"` to each
//! record's CID (a DAG-CBOR map with tag-42 CID links, keys lexicographic).
//! Record blocks follow in the same lexicographic order as their index
//! entries. A consumer validates the stream without buffering the repo:
//!
//! 1. Verify the commit's signature and MAC (the caller resolves the author's
//!    key and runs [`crate::commit::verify_commit`]); its `hash` is now trusted.
//! 2. Fold each index entry into a running [`LtHash`] and compare against the
//!    trusted hash, authenticating the whole index before reading any record.
//! 3. Verify each record block against its index CID as it streams past.

use std::collections::BTreeMap;

use iroh_car::{CarHeader, CarReader, CarWriter};
use lexicon_cid::multihash::Multihash;
use lexicon_cid::Cid;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::error::{Result, SpaceError};
use crate::lthash::LtHash;
use crate::types::SignedCommit;

const SHA2_256: u64 = 0x12;
const DAG_CBOR: u64 = 0x71;

fn car_err(e: iroh_car::Error) -> SpaceError {
    SpaceError::Car(e.to_string())
}

fn codec_err<E: std::fmt::Display>(e: E) -> SpaceError {
    SpaceError::Decode(e.to_string())
}

fn cid_for_block(codec: u64, bytes: &[u8]) -> Cid {
    let digest = Sha256::digest(bytes);
    let multihash = Multihash::wrap(SHA2_256, &digest).expect("sha256 digest fits in multihash");
    Cid::new_v1(codec, multihash)
}

fn dag_cbor_block<T: Serialize>(value: &T) -> Result<(Cid, Vec<u8>)> {
    let bytes = serde_ipld_dagcbor::to_vec(value).map_err(codec_err)?;
    let cid = cid_for_block(DAG_CBOR, &bytes);
    Ok((cid, bytes))
}

/// Serialize a permissioned repo to a CARv1 stream: two roots (signed commit,
/// then index), followed by the record blocks in index order. `blocks`
/// resolves each entry's CID to its record bytes; a `None` is a hard error
/// since the serialization must be complete. Returns the underlying writer.
pub async fn write_repo_car<W, F>(
    writer: W,
    commit: &SignedCommit,
    entries: &BTreeMap<String, Cid>,
    blocks: F,
) -> Result<W>
where
    W: AsyncWrite + Send + Unpin,
    F: Fn(&Cid) -> Option<Vec<u8>>,
{
    let (commit_cid, commit_bytes) = dag_cbor_block(commit)?;
    let (index_cid, index_bytes) = dag_cbor_block(entries)?;
    let header = CarHeader::new_v1(vec![commit_cid, index_cid]);
    let mut car = CarWriter::new(header, writer);
    car.write(commit_cid, commit_bytes).await.map_err(car_err)?;
    car.write(index_cid, index_bytes).await.map_err(car_err)?;
    for (path, cid) in entries {
        let bytes = blocks(cid).ok_or_else(|| SpaceError::MissingBlock(path.clone()))?;
        car.write(*cid, bytes).await.map_err(car_err)?;
    }
    car.finish().await.map_err(car_err)
}

/// [`write_repo_car`] into an in-memory buffer.
pub async fn repo_car_bytes<F>(
    commit: &SignedCommit,
    entries: &BTreeMap<String, Cid>,
    blocks: F,
) -> Result<Vec<u8>>
where
    F: Fn(&Cid) -> Option<Vec<u8>>,
{
    let cursor = std::io::Cursor::new(Vec::new());
    let cursor = write_repo_car(cursor, commit, entries, blocks).await?;
    Ok(cursor.into_inner())
}

/// Streaming consumer of a permissioned repo CAR.
///
/// [`RepoCarValidator::new`] parses the header and the commit root. The caller
/// then verifies the exposed [`commit`](Self::commit) (sig + MAC) with the
/// author's resolved key, and passes the now-trusted hash to
/// [`into_records`](Self::into_records), which authenticates the index against
/// it and validates each record block as it streams past.
#[derive(Debug)]
pub struct RepoCarValidator<R: AsyncRead + Send + Unpin> {
    reader: CarReader<R>,
    index_root: Cid,
    commit: SignedCommit,
}

impl<R: AsyncRead + Send + Unpin> RepoCarValidator<R> {
    pub async fn new(source: R) -> Result<Self> {
        let mut reader = CarReader::new(source).await.map_err(car_err)?;
        let roots = reader.header().roots().to_vec();
        if roots.len() != 2 {
            return Err(SpaceError::RootCountMismatch(roots.len()));
        }
        let (_, bytes) = reader
            .next_block()
            .await
            .map_err(car_err)?
            .ok_or_else(|| SpaceError::MissingBlock("commit".into()))?;
        if cid_for_block(DAG_CBOR, &bytes) != roots[0] {
            return Err(SpaceError::BlockCidMismatch("commit".into()));
        }
        let commit: SignedCommit = serde_ipld_dagcbor::from_slice(&bytes).map_err(codec_err)?;
        Ok(Self {
            reader,
            index_root: roots[1],
            commit,
        })
    }

    /// The decoded commit root. Verify it via [`crate::commit::verify_commit`]
    /// before trusting its `hash`.
    pub fn commit(&self) -> &SignedCommit {
        &self.commit
    }

    /// Consume the index and record blocks, validating against `trusted_hash`
    /// (the commit `hash`, after the caller verified sig + MAC). Returns the
    /// validated records as `(path, cid, bytes)` so a syncer can diff/rebuild.
    pub async fn into_records(
        mut self,
        trusted_hash: &[u8],
    ) -> Result<Vec<(String, Cid, Vec<u8>)>> {
        let (_, bytes) = self
            .reader
            .next_block()
            .await
            .map_err(car_err)?
            .ok_or_else(|| SpaceError::MissingBlock("index".into()))?;
        if cid_for_block(DAG_CBOR, &bytes) != self.index_root {
            return Err(SpaceError::BlockCidMismatch("index".into()));
        }
        let index: BTreeMap<String, Cid> =
            serde_ipld_dagcbor::from_slice(&bytes).map_err(codec_err)?;

        let mut lth = LtHash::new();
        for (path, cid) in &index {
            lth.add(&format!("{path}/{cid}"));
        }
        if lth.hash().as_slice() != trusted_hash {
            return Err(SpaceError::IndexHashMismatch);
        }

        let mut records = Vec::with_capacity(index.len());
        for (path, expected) in index {
            let (cid, bytes) = self
                .reader
                .next_block()
                .await
                .map_err(car_err)?
                .ok_or_else(|| SpaceError::MissingBlock(path.clone()))?;
            if cid != expected {
                return Err(SpaceError::BlockOrderViolation(path));
            }
            if cid_for_block(expected.codec(), &bytes) != expected {
                return Err(SpaceError::BlockCidMismatch(path));
            }
            records.push((path, expected, bytes));
        }
        if self.reader.next_block().await.map_err(car_err)?.is_some() {
            return Err(SpaceError::BlockOrderViolation(
                "trailing block after index entries".into(),
            ));
        }
        Ok(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commit::{build_ctx, compute_mac, verify_commit};
    use crate::lthash::element;
    use secp256k1::{Message, PublicKey, Secp256k1, SecretKey};
    use serde_bytes::ByteBuf;
    use std::io::Cursor;

    const SPACE: &str = "at://did:plc:authority/space/community.blacksky.feed/main";
    const AUTHOR: &str = "did:plc:author";
    const REV: &str = "3rev";
    const IKM: [u8; 32] = [9u8; 32];

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

    fn rec(text: &str) -> (Cid, Vec<u8>) {
        let mut m = BTreeMap::new();
        m.insert("text".to_string(), text.to_string());
        dag_cbor_block(&m).unwrap()
    }

    fn signed_commit(a: &Author, entries: &BTreeMap<String, Cid>) -> SignedCommit {
        let mut lth = LtHash::new();
        for (path, cid) in entries {
            let (collection, rkey) = path.split_once('/').unwrap();
            lth.add(&element(collection, rkey, &cid.to_string()));
        }
        let hash = lth.hash();
        let ctx = build_ctx(SPACE, AUTHOR, REV, &IKM);
        let digest = Sha256::digest(&ctx);
        let msg = Message::from_digest_slice(&digest).unwrap();
        let mut sig = a.secret.sign_ecdsa(msg);
        sig.normalize_s();
        let mac = compute_mac(&IKM, &ctx, &hash).unwrap();
        SignedCommit {
            ver: 1,
            hash: ByteBuf::from(hash.to_vec()),
            ikm: ByteBuf::from(IKM.to_vec()),
            sig: ByteBuf::from(sig.serialize_compact().to_vec()),
            mac: ByteBuf::from(mac.to_vec()),
            rev: REV.to_string(),
        }
    }

    struct Fixture {
        author: Author,
        commit: SignedCommit,
        entries: BTreeMap<String, Cid>,
        store: BTreeMap<Cid, Vec<u8>>,
    }

    fn fixture() -> Fixture {
        let author = author();
        let mut entries = BTreeMap::new();
        let mut store = BTreeMap::new();
        for (path, text) in [
            ("community.blacksky.feed.like/3kc", "like one"),
            ("community.blacksky.feed.post/3ka", "post one"),
            ("community.blacksky.feed.post/3kb", "post two"),
        ] {
            let (cid, bytes) = rec(text);
            entries.insert(path.to_string(), cid);
            store.insert(cid, bytes);
        }
        let commit = signed_commit(&author, &entries);
        Fixture {
            author,
            commit,
            entries,
            store,
        }
    }

    async fn car_of(f: &Fixture) -> Vec<u8> {
        let store = f.store.clone();
        repo_car_bytes(&f.commit, &f.entries, move |cid| store.get(cid).cloned())
            .await
            .unwrap()
    }

    async fn custom_car(roots: Vec<Cid>, blocks: Vec<(Cid, Vec<u8>)>) -> Vec<u8> {
        let header = CarHeader::new_v1(roots);
        let mut w = CarWriter::new(header, Cursor::new(Vec::new()));
        w.write_header().await.unwrap();
        for (cid, bytes) in blocks {
            w.write(cid, bytes).await.unwrap();
        }
        w.finish().await.unwrap().into_inner()
    }

    /// Full write -> validate roundtrip with real commit verification, plus a
    /// golden vector pinning the CAR serialization byte-for-byte.
    #[tokio::test]
    async fn roundtrip_and_golden_vector() {
        let f = fixture();
        let car = car_of(&f).await;
        assert_eq!(
            hex::encode(Sha256::digest(&car)),
            "fcefb95637f8ab8a3965c68e3ce530c7e5549cc013e8506f98baae6df846fb3b"
        );

        let v = RepoCarValidator::new(car.as_slice()).await.unwrap();
        assert!(format!("{v:?}").contains("RepoCarValidator"));
        let commit = v.commit().clone();
        assert_eq!(commit, f.commit);
        verify_commit(
            &f.author.did_key,
            SPACE,
            AUTHOR,
            &commit.rev,
            &commit.ikm,
            &commit.sig,
            &commit.mac,
            &commit.hash,
        )
        .unwrap();
        let records = v.into_records(&commit.hash).await.unwrap();
        assert_eq!(records.len(), 3);
        for ((path, cid, bytes), (want_path, want_cid)) in records.iter().zip(f.entries.iter()) {
            assert_eq!(path, want_path);
            assert_eq!(cid, want_cid);
            assert_eq!(bytes, f.store.get(want_cid).unwrap());
        }
    }

    fn no_blocks(_: &Cid) -> Option<Vec<u8>> {
        None
    }

    #[tokio::test]
    async fn empty_repo_roundtrip() {
        let author = author();
        let entries = BTreeMap::new();
        let commit = signed_commit(&author, &entries);
        let car = repo_car_bytes(&commit, &entries, no_blocks).await.unwrap();
        let v = RepoCarValidator::new(car.as_slice()).await.unwrap();
        let trusted = v.commit().hash.clone();
        let records = v.into_records(&trusted).await.unwrap();
        assert!(records.is_empty());
    }

    #[tokio::test]
    async fn write_repo_car_missing_block_rejected() {
        let f = fixture();
        let err = repo_car_bytes(&f.commit, &f.entries, no_blocks)
            .await
            .unwrap_err();
        assert!(matches!(err, SpaceError::MissingBlock(_)));
    }

    #[derive(Debug)]
    struct FailWriter;
    impl AsyncWrite for FailWriter {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            std::task::Poll::Ready(Ok(buf.len()))
        }
        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Err(std::io::Error::other("flush refused")))
        }
        fn poll_shutdown(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn write_failure_surfaces_car_error() {
        let f = fixture();
        let store = f.store.clone();
        let err = write_repo_car(FailWriter, &f.commit, &f.entries, move |cid| {
            store.get(cid).cloned()
        })
        .await
        .unwrap_err();
        assert!(matches!(err, SpaceError::Car(_)));

        use tokio::io::AsyncWriteExt;
        let mut writer = FailWriter;
        writer.flush().await.unwrap_err();
        writer.shutdown().await.unwrap();
        assert_eq!(format!("{writer:?}"), "FailWriter");
    }

    #[test]
    fn unencodable_block_rejected() {
        // DAG-CBOR cannot encode u128, exercising the encode-side error map.
        assert!(matches!(
            dag_cbor_block(&u128::MAX).unwrap_err(),
            SpaceError::Decode(_)
        ));
    }

    #[tokio::test]
    async fn garbage_header_rejected() {
        let err = RepoCarValidator::new(&[0xFFu8; 8][..]).await.unwrap_err();
        assert!(matches!(err, SpaceError::Car(_)));
    }

    #[tokio::test]
    async fn wrong_root_counts_rejected() {
        let f = fixture();
        let (commit_cid, commit_bytes) = dag_cbor_block(&f.commit).unwrap();
        let (index_cid, _) = dag_cbor_block(&f.entries).unwrap();

        let one = custom_car(vec![commit_cid], vec![(commit_cid, commit_bytes.clone())]).await;
        assert!(matches!(
            RepoCarValidator::new(one.as_slice()).await.unwrap_err(),
            SpaceError::RootCountMismatch(1)
        ));

        let three = custom_car(
            vec![commit_cid, index_cid, commit_cid],
            vec![(commit_cid, commit_bytes)],
        )
        .await;
        assert!(matches!(
            RepoCarValidator::new(three.as_slice()).await.unwrap_err(),
            SpaceError::RootCountMismatch(3)
        ));
    }

    #[tokio::test]
    async fn missing_commit_block_rejected() {
        let f = fixture();
        let (commit_cid, _) = dag_cbor_block(&f.commit).unwrap();
        let (index_cid, _) = dag_cbor_block(&f.entries).unwrap();
        let car = custom_car(vec![commit_cid, index_cid], vec![]).await;
        assert!(matches!(
            RepoCarValidator::new(car.as_slice()).await.unwrap_err(),
            SpaceError::MissingBlock(_)
        ));
    }

    #[tokio::test]
    async fn commit_block_cid_mismatch_rejected() {
        let f = fixture();
        let (index_cid, _) = dag_cbor_block(&f.entries).unwrap();
        // Root 0 claims the index CID but the first block carries commit bytes.
        let (_, commit_bytes) = dag_cbor_block(&f.commit).unwrap();
        let car = custom_car(vec![index_cid, index_cid], vec![(index_cid, commit_bytes)]).await;
        assert!(matches!(
            RepoCarValidator::new(car.as_slice()).await.unwrap_err(),
            SpaceError::BlockCidMismatch(_)
        ));
    }

    #[tokio::test]
    async fn commit_block_decode_failure_rejected() {
        let f = fixture();
        let (not_commit_cid, not_commit_bytes) = dag_cbor_block(&"not a commit").unwrap();
        let (index_cid, _) = dag_cbor_block(&f.entries).unwrap();
        let car = custom_car(
            vec![not_commit_cid, index_cid],
            vec![(not_commit_cid, not_commit_bytes)],
        )
        .await;
        assert!(matches!(
            RepoCarValidator::new(car.as_slice()).await.unwrap_err(),
            SpaceError::Decode(_)
        ));
    }

    #[tokio::test]
    async fn missing_index_block_rejected() {
        let f = fixture();
        let (commit_cid, commit_bytes) = dag_cbor_block(&f.commit).unwrap();
        let (index_cid, _) = dag_cbor_block(&f.entries).unwrap();
        let car = custom_car(
            vec![commit_cid, index_cid],
            vec![(commit_cid, commit_bytes)],
        )
        .await;
        let v = RepoCarValidator::new(car.as_slice()).await.unwrap();
        let trusted = v.commit().hash.clone();
        assert!(matches!(
            v.into_records(&trusted).await.unwrap_err(),
            SpaceError::MissingBlock(_)
        ));
    }

    #[tokio::test]
    async fn index_block_cid_mismatch_rejected() {
        let f = fixture();
        let (commit_cid, commit_bytes) = dag_cbor_block(&f.commit).unwrap();
        let (index_cid, _) = dag_cbor_block(&f.entries).unwrap();
        // Second block carries commit bytes again instead of the index.
        let car = custom_car(
            vec![commit_cid, index_cid],
            vec![
                (commit_cid, commit_bytes.clone()),
                (index_cid, commit_bytes),
            ],
        )
        .await;
        let v = RepoCarValidator::new(car.as_slice()).await.unwrap();
        let trusted = v.commit().hash.clone();
        assert!(matches!(
            v.into_records(&trusted).await.unwrap_err(),
            SpaceError::BlockCidMismatch(_)
        ));
    }

    #[tokio::test]
    async fn index_block_decode_failure_rejected() {
        let f = fixture();
        let (commit_cid, commit_bytes) = dag_cbor_block(&f.commit).unwrap();
        let (bad_index_cid, bad_index_bytes) = dag_cbor_block(&"not an index").unwrap();
        let car = custom_car(
            vec![commit_cid, bad_index_cid],
            vec![(commit_cid, commit_bytes), (bad_index_cid, bad_index_bytes)],
        )
        .await;
        let v = RepoCarValidator::new(car.as_slice()).await.unwrap();
        let trusted = v.commit().hash.clone();
        assert!(matches!(
            v.into_records(&trusted).await.unwrap_err(),
            SpaceError::Decode(_)
        ));
    }

    #[tokio::test]
    async fn commit_hash_not_matching_index_rejected() {
        let f = fixture();
        // Commit over a different record set than the serialized index.
        let mut other_entries = f.entries.clone();
        other_entries.remove("community.blacksky.feed.like/3kc");
        let commit = signed_commit(&f.author, &other_entries);
        let store = f.store.clone();
        let car = repo_car_bytes(&commit, &f.entries, move |cid| store.get(cid).cloned())
            .await
            .unwrap();
        let v = RepoCarValidator::new(car.as_slice()).await.unwrap();
        let trusted = v.commit().hash.clone();
        assert!(matches!(
            v.into_records(&trusted).await.unwrap_err(),
            SpaceError::IndexHashMismatch
        ));
    }

    #[tokio::test]
    async fn wrong_index_cid_for_record_rejected() {
        let f = fixture();
        // Index (and commit) claim a different CID for one path than the bytes
        // actually served for it: the index authenticates, the block does not.
        let (bogus_cid, _) = dag_cbor_block(&"someone else's record").unwrap();
        let mut entries = f.entries.clone();
        let victim = "community.blacksky.feed.post/3ka".to_string();
        let real_cid = entries.insert(victim.clone(), bogus_cid).unwrap();
        let commit = signed_commit(&f.author, &entries);
        let store = f.store.clone();
        let car = repo_car_bytes(&commit, &entries, move |cid| {
            if *cid == bogus_cid {
                store.get(&real_cid).cloned()
            } else {
                store.get(cid).cloned()
            }
        })
        .await
        .unwrap();
        let v = RepoCarValidator::new(car.as_slice()).await.unwrap();
        let trusted = v.commit().hash.clone();
        assert!(matches!(
            v.into_records(&trusted).await.unwrap_err(),
            SpaceError::BlockCidMismatch(path) if path == victim
        ));
    }

    #[tokio::test]
    async fn flipped_record_byte_rejected() {
        let f = fixture();
        let (commit_cid, commit_bytes) = dag_cbor_block(&f.commit).unwrap();
        let (index_cid, index_bytes) = dag_cbor_block(&f.entries).unwrap();
        let mut blocks = vec![(commit_cid, commit_bytes), (index_cid, index_bytes)];
        for (i, cid) in f.entries.values().enumerate() {
            let mut bytes = f.store.get(cid).cloned().unwrap();
            if i == 1 {
                bytes[0] ^= 0xFF;
            }
            blocks.push((*cid, bytes));
        }
        let car = custom_car(vec![commit_cid, index_cid], blocks).await;
        let v = RepoCarValidator::new(car.as_slice()).await.unwrap();
        let trusted = v.commit().hash.clone();
        assert!(matches!(
            v.into_records(&trusted).await.unwrap_err(),
            SpaceError::BlockCidMismatch(_)
        ));
    }

    #[tokio::test]
    async fn reordered_record_blocks_rejected() {
        let f = fixture();
        let (commit_cid, commit_bytes) = dag_cbor_block(&f.commit).unwrap();
        let (index_cid, index_bytes) = dag_cbor_block(&f.entries).unwrap();
        let mut records: Vec<(Cid, Vec<u8>)> = f
            .entries
            .values()
            .map(|cid| (*cid, f.store.get(cid).cloned().unwrap()))
            .collect();
        records.swap(0, 1);
        let mut blocks = vec![(commit_cid, commit_bytes), (index_cid, index_bytes)];
        blocks.extend(records);
        let car = custom_car(vec![commit_cid, index_cid], blocks).await;
        let v = RepoCarValidator::new(car.as_slice()).await.unwrap();
        let trusted = v.commit().hash.clone();
        assert!(matches!(
            v.into_records(&trusted).await.unwrap_err(),
            SpaceError::BlockOrderViolation(_)
        ));
    }

    #[tokio::test]
    async fn missing_record_block_rejected() {
        let f = fixture();
        let (commit_cid, commit_bytes) = dag_cbor_block(&f.commit).unwrap();
        let (index_cid, index_bytes) = dag_cbor_block(&f.entries).unwrap();
        let mut blocks = vec![(commit_cid, commit_bytes), (index_cid, index_bytes)];
        for cid in f.entries.values().take(2) {
            blocks.push((*cid, f.store.get(cid).cloned().unwrap()));
        }
        let car = custom_car(vec![commit_cid, index_cid], blocks).await;
        let v = RepoCarValidator::new(car.as_slice()).await.unwrap();
        let trusted = v.commit().hash.clone();
        assert!(matches!(
            v.into_records(&trusted).await.unwrap_err(),
            SpaceError::MissingBlock(_)
        ));
    }

    #[tokio::test]
    async fn extra_trailing_block_rejected() {
        let f = fixture();
        let (commit_cid, commit_bytes) = dag_cbor_block(&f.commit).unwrap();
        let (index_cid, index_bytes) = dag_cbor_block(&f.entries).unwrap();
        let mut blocks = vec![(commit_cid, commit_bytes), (index_cid, index_bytes)];
        for cid in f.entries.values() {
            blocks.push((*cid, f.store.get(cid).cloned().unwrap()));
        }
        let (extra_cid, extra_bytes) = rec("stowaway");
        blocks.push((extra_cid, extra_bytes));
        let car = custom_car(vec![commit_cid, index_cid], blocks).await;
        let v = RepoCarValidator::new(car.as_slice()).await.unwrap();
        let trusted = v.commit().hash.clone();
        assert!(matches!(
            v.into_records(&trusted).await.unwrap_err(),
            SpaceError::BlockOrderViolation(_)
        ));
    }
}
