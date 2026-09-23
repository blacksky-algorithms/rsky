//! Builders for signed `subscribeRepos` frames used by validator and migration tests.
use cid::Cid;
use p256::ecdsa::signature::Signer as _;
use sha2::{Digest, Sha256};

use rsky_common::tid::TID;

use crate::types::Cursor;
use crate::validator::event::{
    Commit, SubscribeReposAccount, SubscribeReposCommit, SubscribeReposCommitOperation,
    SubscribeReposEvent, SubscribeReposIdentity, SubscribeReposSync,
};

pub const P256_DID_PREFIX: &[u8] = &[0x80, 0x24];
pub const TEST_TIME: &str = "2026-09-23T12:00:00.000Z";

pub fn test_time() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(TEST_TIME).unwrap().with_timezone(&chrono::Utc)
}

pub struct TestKey {
    pub signing: p256::ecdsa::SigningKey,
    pub did_key: [u8; 35],
}

pub fn test_key(seed: u8) -> TestKey {
    let signing = p256::ecdsa::SigningKey::from_slice(&[seed; 32]).unwrap();
    let point = signing.verifying_key().to_encoded_point(true);
    let mut did_key = [0u8; 35];
    did_key[0..2].copy_from_slice(P256_DID_PREFIX);
    did_key[2..].copy_from_slice(point.as_bytes());
    TestKey { signing, did_key }
}

fn dag_cbor_cid(block: &[u8]) -> Cid {
    let digest = Sha256::digest(block);
    let mh = cid::multihash::Multihash::<64>::wrap(0x12, &digest).unwrap();
    Cid::new_v1(0x71, mh)
}

fn varint(mut n: u64, out: &mut Vec<u8>) {
    loop {
        let byte = (n & 0x7f) as u8;
        n >>= 7;
        if n == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
}

/// Minimal `CARv1`: header `{version: 1, roots: [root]}` then one block per entry.
pub fn car(root: Cid, blocks: &[(Cid, Vec<u8>)]) -> Vec<u8> {
    #[derive(serde::Serialize)]
    struct Header {
        roots: Vec<Cid>,
        version: u64,
    }
    let header = serde_ipld_dagcbor::to_vec(&Header { roots: vec![root], version: 1 }).unwrap();
    let mut out = Vec::new();
    varint(header.len() as u64, &mut out);
    out.extend_from_slice(&header);
    for (cid, block) in blocks {
        let cid_bytes = cid.to_bytes();
        varint((cid_bytes.len() + block.len()) as u64, &mut out);
        out.extend_from_slice(&cid_bytes);
        out.extend_from_slice(block);
    }
    out
}

pub struct CommitSpec<'a> {
    pub did: &'a str,
    pub rev: &'a str,
    pub seq: u64,
    pub data: Cid,
    pub prev_data: Option<Cid>,
    pub key: &'a TestKey,
    pub sig_override: Option<Vec<u8>>,
    pub too_big: bool,
}

pub fn empty_cid() -> Cid {
    Cid::try_from("bafyreigbtj4x7ip5legnfznufuopl4sg4knzc2cof6duas4b3q2fy6swua").unwrap()
}

pub fn data_cid(tag: u8) -> Cid {
    dag_cbor_cid(&[tag])
}

#[derive(serde::Serialize)]
struct SignedCommit<'a> {
    did: &'a str,
    rev: &'a TID,
    data: &'a Cid,
    prev: Option<Cid>,
    version: u8,
    #[serde(with = "serde_bytes")]
    sig: &'a [u8],
}

/// A signed commit frame with an empty op list whose CAR carries only the
/// commit block, which is what the validator's envelope checks need.
pub fn commit_frame(spec: &CommitSpec<'_>) -> (Vec<u8>, Cid) {
    let mut commit = Commit {
        did: spec.did.to_owned(),
        rev: TID::new(spec.rev.to_owned()).unwrap(),
        data: spec.data,
        prev: None,
        version: 3,
        sig: Vec::new(),
    };
    let unsigned = serde_ipld_dagcbor::to_vec(&commit).unwrap();
    // atproto requires low-S; the deterministic signer does not normalize
    let sig: p256::ecdsa::Signature = spec.key.signing.sign(&unsigned);
    let sig = sig.normalize_s().unwrap_or(sig);
    commit.sig = spec.sig_override.clone().unwrap_or_else(|| sig.to_vec());
    let block = serde_ipld_dagcbor::to_vec(&SignedCommit {
        did: &commit.did,
        rev: &commit.rev,
        data: &commit.data,
        prev: None,
        version: 3,
        sig: &commit.sig,
    })
    .unwrap();
    let head = dag_cbor_cid(&block);
    let blocks = car(head, &[(head, block)]);
    let event = SubscribeReposEvent::Commit(SubscribeReposCommit {
        seq: spec.seq,
        rebase: false,
        too_big: spec.too_big,
        did: spec.did.to_owned(),
        commit: head,
        rev: commit.rev,
        since: None,
        blocks,
        ops: Vec::<SubscribeReposCommitOperation>::new(),
        blobs: Vec::new(),
        prev_data: spec.prev_data,
        time: TEST_TIME.to_owned(),
        time_dt: test_time(),
    });
    (event.serialize(0, Cursor::from(spec.seq)).unwrap(), head)
}

pub fn sync_frame(did: &str, rev: &str, seq: u64, key: &TestKey) -> (Vec<u8>, Cid) {
    let (commit, head) = commit_frame(&CommitSpec {
        did,
        rev,
        seq,
        data: data_cid(9),
        prev_data: None,
        key,
        sig_override: None,
        too_big: false,
    });
    let SubscribeReposEvent::Commit(commit) = SubscribeReposEvent::parse(&commit).unwrap().unwrap()
    else {
        unreachable!()
    };
    let event = SubscribeReposEvent::Sync(SubscribeReposSync {
        seq,
        did: did.to_owned(),
        blocks: commit.blocks,
        rev: commit.rev,
        time: TEST_TIME.to_owned(),
        time_dt: test_time(),
    });
    (event.serialize(0, Cursor::from(seq)).unwrap(), head)
}

pub fn identity_frame(did: &str, seq: u64, handle: Option<&str>) -> Vec<u8> {
    SubscribeReposEvent::Identity(SubscribeReposIdentity {
        seq,
        did: did.to_owned(),
        time: TEST_TIME.to_owned(),
        time_dt: test_time(),
        handle: handle.map(ToOwned::to_owned),
    })
    .serialize(0, Cursor::from(seq))
    .unwrap()
}

pub fn account_frame(did: &str, seq: u64, active: bool) -> Vec<u8> {
    SubscribeReposEvent::Account(SubscribeReposAccount {
        seq,
        did: did.to_owned(),
        time: TEST_TIME.to_owned(),
        time_dt: test_time(),
        active,
        status: None,
    })
    .serialize(0, Cursor::from(seq))
    .unwrap()
}

/// A `#commit` frame whose body decodes but whose CAR is garbage.
pub fn broken_car_frame(did: &str, seq: u64) -> Vec<u8> {
    SubscribeReposEvent::Commit(SubscribeReposCommit {
        seq,
        rebase: false,
        too_big: false,
        did: did.to_owned(),
        commit: empty_cid(),
        rev: TID::new("3mw73ekioet25".to_owned()).unwrap(),
        since: None,
        blocks: vec![1, 2, 3],
        ops: Vec::new(),
        blobs: Vec::new(),
        prev_data: None,
        time: TEST_TIME.to_owned(),
        time_dt: test_time(),
    })
    .serialize(0, Cursor::from(seq))
    .unwrap()
}

pub fn error_frame(name: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    ciborium::ser::into_writer(
        &crate::validator::event::Header { type_: std::borrow::Cow::Borrowed(""), operation_: -1 },
        &mut buf,
    )
    .unwrap();
    serde_ipld_dagcbor::to_writer(&mut buf, &serde_json::json!({"error": name, "message": "m"}))
        .unwrap();
    buf
}

pub fn info_frame(name: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    ciborium::ser::into_writer(
        &crate::validator::event::Header {
            type_: std::borrow::Cow::Borrowed("#info"),
            operation_: 1,
        },
        &mut buf,
    )
    .unwrap();
    serde_ipld_dagcbor::to_writer(&mut buf, &serde_json::json!({"name": name, "message": "m"}))
        .unwrap();
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validator::utils::verify_commit_sig;

    #[test]
    fn commit_frame_round_trips_and_verifies() {
        let key = test_key(7);
        let (frame, head) = commit_frame(&CommitSpec {
            did: "did:plc:test",
            rev: "3mw73ekioet25",
            seq: 5,
            data: data_cid(1),
            prev_data: None,
            key: &key,
            sig_override: None,
            too_big: false,
        });
        let event = SubscribeReposEvent::parse(&frame).unwrap().unwrap();
        let (commit, parsed_head) = event.commit().unwrap().unwrap();
        assert_eq!(parsed_head, head);
        assert!(event.validate(&commit, &parsed_head));
        assert!(verify_commit_sig(&commit, &key.did_key).unwrap());
        assert!(!verify_commit_sig(&commit, &test_key(8).did_key).unwrap());
        let (sync, sync_head) = sync_frame("did:plc:test", "3mw73ekioet26", 6, &key);
        let event = SubscribeReposEvent::parse(&sync).unwrap().unwrap();
        let (sync_commit, parsed_head) = event.commit().unwrap().unwrap();
        assert_eq!(parsed_head, sync_head);
        assert!(verify_commit_sig(&sync_commit, &key.did_key).unwrap(), "sync commit must verify");
        assert!(
            SubscribeReposEvent::parse(&identity_frame("did:plc:t", 1, Some("h")))
                .unwrap()
                .is_some()
        );
        assert!(
            SubscribeReposEvent::parse(&account_frame("did:plc:t", 2, true)).unwrap().is_some()
        );
        let broken =
            SubscribeReposEvent::parse(&broken_car_frame("did:plc:t", 3)).unwrap().unwrap();
        assert!(broken.commit().is_err());
        assert!(matches!(
            SubscribeReposEvent::parse_frame(&error_frame("FutureCursor")).unwrap(),
            crate::validator::event::Frame::Error { .. }
        ));
        assert!(matches!(
            SubscribeReposEvent::parse_frame(&info_frame("OutdatedCursor")).unwrap(),
            crate::validator::event::Frame::Info { .. }
        ));
    }
}
