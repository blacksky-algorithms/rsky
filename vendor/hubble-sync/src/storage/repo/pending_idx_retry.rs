//! Index over pending identity resolution retries
//!
//! "pq|"||<due_millis: u64_be>||<did> => []

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use metrics::counter;

use super::{DecodeError, LoadError, PREFIX_REPO_PENDING_IDX_RETRY};
use crate::metrics::{
    PENDING_IDENTITY_QUEUE_DEQUEUED_TOTAL, PENDING_IDENTITY_QUEUE_ENQUEUED_TOTAL,
};
use crate::{Did, StorageBatch, StorageEngine, StorageError};

pub struct PendingIdentityQueueEntry {
    pub did: Did,
    pub due: SystemTime,
}

impl PendingIdentityQueueEntry {
    fn key(did: &Did, due: SystemTime) -> Vec<u8> {
        let due_ms = due
            .duration_since(UNIX_EPOCH)
            .expect("post-epoch pending due")
            .as_millis() as u64;
        let did_bytes = did.as_str().as_bytes();
        let mut k = Vec::with_capacity(PREFIX_REPO_PENDING_IDX_RETRY.len() + 8 + did_bytes.len());
        k.extend_from_slice(PREFIX_REPO_PENDING_IDX_RETRY);
        k.extend_from_slice(&due_ms.to_be_bytes());
        k.extend_from_slice(did_bytes);
        k
    }

    fn decode_key(k_unprefixed: &[u8]) -> Result<Self, DecodeError> {
        let (due_bytes, did_bytes) = k_unprefixed
            .split_first_chunk::<8>()
            .ok_or(DecodeError::InputTooShort)?;
        let due_ms = u64::from_be_bytes(*due_bytes);
        let due = UNIX_EPOCH + Duration::from_millis(due_ms);
        let did = str::from_utf8(did_bytes)
            .map_err(|err| DecodeError::NotUtf8 { what: "did", err })?
            .try_into()
            .map_err(DecodeError::BadDid)?;
        Ok(Self { did, due })
    }

    pub fn insert<E: StorageError, B: StorageBatch<E>>(&self, batch: &mut B) {
        batch.put_queue(&Self::key(&self.did, self.due), &[]);
        counter!(PENDING_IDENTITY_QUEUE_ENQUEUED_TOTAL).increment(1);
    }

    pub fn delete<E: StorageError, B: StorageBatch<E>>(self, batch: &mut B) {
        batch.delete_queue(&Self::key(&self.did, self.due));
        counter!(PENDING_IDENTITY_QUEUE_DEQUEUED_TOTAL).increment(1);
    }

    pub fn scan<S: StorageEngine>(
        storage: &S,
    ) -> impl Iterator<Item = Result<Self, LoadError<S::Error>>> {
        storage
            .scan_from_queue(PREFIX_REPO_PENDING_IDX_RETRY, &[])
            .map(|r| {
                let (k, _) = r.map_err(LoadError::Storage)?;
                Ok(Self::decode_key(&k)?)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::engine::mem::MemEngine;

    fn plc_did(label: &str) -> Did {
        let mut s = label.to_string();
        while s.len() < 24 {
            s.push('a');
        }
        s.truncate(24);
        Did::new(format!("did:plc:{s}")).expect("valid plc did")
    }

    fn t(ms: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(ms)
    }

    fn entry(label: &str, due_ms: u64) -> PendingIdentityQueueEntry {
        PendingIdentityQueueEntry {
            did: plc_did(label),
            due: t(due_ms),
        }
    }

    #[test]
    fn scan_empty_yields_none() {
        let eng = MemEngine::new();
        let mut iter = PendingIdentityQueueEntry::scan(&eng);
        assert!(iter.next().is_none());
    }

    #[test]
    fn insert_then_scan_roundtrips_one_entry() {
        let eng = MemEngine::new();
        let mut b = eng.batch();
        entry("abc", 1_000).insert(&mut b);
        b.commit().unwrap();

        let items: Vec<_> = PendingIdentityQueueEntry::scan(&eng)
            .collect::<Result<Vec<_>, _>>()
            .expect("scan ok");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].did, plc_did("abc"));
        assert_eq!(items[0].due, t(1_000));
    }

    #[test]
    fn scan_returns_entries_in_time_sorted_order() {
        let eng = MemEngine::new();
        let mut b = eng.batch();
        // insert out of order to verify sort happens on read
        entry("b", 5_000).insert(&mut b);
        entry("a", 1_000).insert(&mut b);
        entry("c", 3_000).insert(&mut b);
        b.commit().unwrap();

        let items: Vec<_> = PendingIdentityQueueEntry::scan(&eng)
            .collect::<Result<Vec<_>, _>>()
            .expect("scan ok");
        let dues: Vec<_> = items.iter().map(|e| e.due).collect();
        assert_eq!(dues, vec![t(1_000), t(3_000), t(5_000)]);
    }

    #[test]
    fn delete_removes_the_entry() {
        let eng = MemEngine::new();
        let mut b = eng.batch();
        entry("a", 1_000).insert(&mut b);
        entry("b", 2_000).insert(&mut b);
        b.commit().unwrap();

        let mut b = eng.batch();
        entry("a", 1_000).delete(&mut b);
        b.commit().unwrap();

        let items: Vec<_> = PendingIdentityQueueEntry::scan(&eng)
            .collect::<Result<Vec<_>, _>>()
            .expect("scan ok");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].did, plc_did("b"));
    }

    #[test]
    fn scan_propagates_decode_error() {
        let eng = MemEngine::new();
        let mut b = eng.batch();
        // truncate the time prefix: less than 8 bytes after the keyspace prefix
        let mut bad_key = Vec::new();
        bad_key.extend_from_slice(PREFIX_REPO_PENDING_IDX_RETRY);
        bad_key.extend_from_slice(b"short");
        b.put_queue(&bad_key, &[]);
        b.commit().unwrap();

        let result = PendingIdentityQueueEntry::scan(&eng)
            .next()
            .expect("got an item");
        match result {
            Ok(_) => panic!("expected decode error, got entry"),
            Err(LoadError::Decode(DecodeError::InputTooShort)) => {}
            Err(other) => panic!("wrong error variant: {other:?}"),
        }
    }
}
