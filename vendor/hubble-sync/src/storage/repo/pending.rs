//! DIDs we've seen but couldn't resolve
//!
//! (or is it haven't-yet-resolved? will we always write early?)
//!
//! when resolution succeeds, deleted in an atomic batch with the repo info put
//!
//! "pi|"||<DID> => <DbPendingValue:cbor>

use std::time::SystemTime;

use dasl::drisl;
use serde::{Deserialize, Serialize};

use crate::identity::pending_resolve_backoff;
use crate::storage::repo::{AccountStatus, AccountStatusSource};
use crate::{Did, StorageBatch, StorageEngine, StorageError};

use super::{LoadError, PREFIX_REPO_PENDING, PendingIdentityQueueEntry, unix_ms_u64};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DbPendingValue {
    #[serde(with = "unix_ms_u64")]
    first_seen: SystemTime,
    #[serde(with = "unix_ms_u64")]
    next_try_at: SystemTime,
    attempts: u32,
    last_error: Option<String>,
    moderation: Option<(AccountStatus, AccountStatusSource)>,
}

#[derive(Debug, Clone)]
pub struct PendingIdentity {
    pub did: Did,
    first_seen: SystemTime,
    next_try_at: SystemTime,
    attempts: u32,
    last_error: Option<String>,
    moderation: Option<(AccountStatus, AccountStatusSource)>,
}

impl PendingIdentity {
    pub fn new(did: Did, now: SystemTime) -> Self {
        Self {
            did,
            first_seen: now,
            next_try_at: now,
            attempts: 0,
            last_error: None,
            moderation: None,
        }
    }

    pub fn enqueue_new<E: StorageError, B: StorageBatch<E>>(
        did: Did,
        moderation: Option<(AccountStatus, AccountStatusSource)>,
        now: SystemTime,
        batch: &mut B,
    ) {
        let mut p = Self::new(did, now);
        p.moderation = moderation;
        p.queue_entry().insert(batch);
        p.store(batch);
    }

    pub fn is_ready(&self, now: SystemTime) -> bool {
        now >= self.next_try_at
    }

    pub fn next_try_at(&self) -> SystemTime {
        self.next_try_at
    }

    pub fn moderation(&self) -> Option<&(AccountStatus, AccountStatusSource)> {
        self.moderation.as_ref()
    }

    fn key(did: &Did) -> Vec<u8> {
        let did_bytes = did.as_str().as_bytes();
        let mut k = Vec::with_capacity(PREFIX_REPO_PENDING.len() + did_bytes.len());
        k.extend_from_slice(PREFIX_REPO_PENDING);
        k.extend_from_slice(did_bytes);
        k
    }

    fn queue_entry(&self) -> PendingIdentityQueueEntry {
        PendingIdentityQueueEntry {
            did: self.did.clone(),
            due: self.next_try_at,
        }
    }

    pub fn load<S: StorageEngine>(
        storage: &S,
        did: &Did,
    ) -> Result<Option<Self>, LoadError<S::Error>> {
        let Some(bytes) = storage.get(&Self::key(did)).map_err(LoadError::Storage)? else {
            return Ok(None);
        };
        let db_val: DbPendingValue = drisl::from_slice(&bytes)?;
        Ok(Some(Self {
            did: did.clone(),
            first_seen: db_val.first_seen,
            attempts: db_val.attempts,
            next_try_at: db_val.next_try_at,
            last_error: db_val.last_error,
            moderation: db_val.moderation,
        }))
    }

    fn store<E: StorageError, B: StorageBatch<E>>(&self, batch: &mut B) {
        let db_val = DbPendingValue {
            first_seen: self.first_seen,
            attempts: self.attempts,
            next_try_at: self.next_try_at,
            last_error: self.last_error.clone(),
            moderation: self.moderation.clone(),
        };
        let bytes = drisl::to_vec(&db_val).expect("drisl to_vec infallibe");
        batch.put(&Self::key(&self.did), &bytes);
    }

    /// pending id resolution failed: get set up for the retry
    ///
    /// insert the retry entry, fix up the queue index (atomically)
    pub fn store_failed<E: StorageError, B: StorageBatch<E>>(
        &mut self,
        reason: &str,
        now: SystemTime,
        batch: &mut B,
    ) {
        let prior = self.queue_entry();

        // bookkeep
        self.attempts = self.attempts.saturating_add(1);
        self.next_try_at = now + pending_resolve_backoff(self.did.method(), self.attempts);
        self.last_error = Some(reason.to_string());

        prior.delete(batch);
        self.queue_entry().insert(batch);
        self.store(batch);
    }

    pub fn delete<E: StorageError, B: StorageBatch<E>>(self, batch: &mut B) {
        self.queue_entry().delete(batch); // pq|
        batch.delete(&Self::key(&self.did)); // pi|
    }

    pub fn reconcile_delete<E: StorageError, B: StorageBatch<E>>(
        entry: PendingIdentityQueueEntry,
        batch: &mut B,
    ) {
        let key = Self::key(&entry.did);
        entry.delete(batch); // pq|
        batch.delete(&key); // pi|
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;
    use crate::storage::engine::mem::MemEngine;
    use crate::storage::repo::PendingIdentityQueueEntry;

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

    fn pq_dids<S: StorageEngine>(eng: &S) -> Vec<Did> {
        PendingIdentityQueueEntry::scan(eng)
            .map(|r| r.expect("decode ok").did)
            .collect()
    }

    #[test]
    fn fresh_is_ready_now() {
        let p = PendingIdentity::new(plc_did("a"), t(1_000));
        assert!(p.is_ready(t(1_000)));
        assert!(p.is_ready(t(10_000)));
    }

    #[test]
    fn fresh_is_not_persisted() {
        // a brand-new in-memory PendingIdentity (attempts==0) has no on-disk
        // state; load returns None even though the in-memory value exists.
        let eng = MemEngine::new();
        let _ = PendingIdentity::new(plc_did("never_stored"), t(1_000));
        assert!(
            PendingIdentity::load(&eng, &plc_did("never_stored"))
                .unwrap()
                .is_none()
        );
        assert!(pq_dids(&eng).is_empty());
    }

    #[test]
    fn store_failed_writes_pi_and_pq() {
        let eng = MemEngine::new();
        let mut p = PendingIdentity::new(plc_did("a"), t(1_000));
        let mut b = eng.batch();
        p.store_failed("oops", t(1_000), &mut b);
        b.commit().unwrap();

        let loaded = PendingIdentity::load(&eng, &plc_did("a"))
            .unwrap()
            .expect("persisted");
        assert_eq!(loaded.attempts, 1);
        assert_eq!(loaded.last_error.as_deref(), Some("oops"));
        // first_seen is from construction, not from the failure
        assert_eq!(loaded.first_seen, t(1_000));
        // next_try_at advanced by the PLC backoff for attempt 1
        assert!(loaded.next_try_at > t(1_000));

        // and exactly one matching pq entry
        let entries: Vec<_> = PendingIdentityQueueEntry::scan(&eng)
            .collect::<Result<Vec<_>, _>>()
            .expect("scan ok");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].did, plc_did("a"));
        assert_eq!(entries[0].due, loaded.next_try_at);
    }

    #[test]
    fn store_failed_twice_advances_pq_no_dup() {
        let eng = MemEngine::new();
        let mut p = PendingIdentity::new(plc_did("a"), t(1_000));

        let mut b = eng.batch();
        p.store_failed("first", t(1_000), &mut b);
        b.commit().unwrap();
        let first_due = p.next_try_at;

        let mut b = eng.batch();
        p.store_failed("second", t(1_000), &mut b);
        b.commit().unwrap();
        let second_due = p.next_try_at;

        assert_eq!(p.attempts, 2);
        assert!(second_due > first_due, "second backoff should be longer");
        assert_eq!(p.last_error.as_deref(), Some("second"));

        // only one pq entry — the old one was deleted, the new one written
        let entries: Vec<_> = PendingIdentityQueueEntry::scan(&eng)
            .collect::<Result<Vec<_>, _>>()
            .expect("scan ok");
        assert_eq!(
            entries.len(),
            1,
            "store_failed should advance pq, not duplicate"
        );
        assert_eq!(entries[0].due, second_due);
    }

    #[test]
    fn delete_after_store_failed_clears_both_pi_and_pq() {
        // delete removes the pi| record and its pq| index entry together, so no
        // orphaned queue entry is left behind (the churn bug this whole change
        // fixes).
        let eng = MemEngine::new();
        let mut p = PendingIdentity::new(plc_did("a"), t(1_000));
        let mut b = eng.batch();
        p.store_failed("oops", t(1_000), &mut b);
        b.commit().unwrap();
        assert_eq!(pq_dids(&eng), vec![plc_did("a")]);

        let mut b = eng.batch();
        p.delete(&mut b);
        b.commit().unwrap();

        assert!(
            PendingIdentity::load(&eng, &plc_did("a"))
                .unwrap()
                .is_none(),
            "pi| should be gone after delete"
        );
        assert!(
            pq_dids(&eng).is_empty(),
            "pq| index entry should also be gone after delete; got: {:?}",
            pq_dids(&eng)
        );
    }

    #[test]
    fn delete_of_fresh_in_memory_pending_is_safe() {
        // Fresh PendingIdentity (never persisted) delete: it now unconditionally
        // targets both pi| and pq|, but neither was ever written, so both
        // deletes are harmless no-ops and the queue stays empty.
        let eng = MemEngine::new();
        let p = PendingIdentity::new(plc_did("a"), t(1_000));

        let mut b = eng.batch();
        p.delete(&mut b);
        b.commit().unwrap();

        assert!(pq_dids(&eng).is_empty());
    }

    #[test]
    fn load_returns_none_for_unknown_did() {
        let eng = MemEngine::new();
        let got = PendingIdentity::load(&eng, &plc_did("ghost")).unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn enqueue_new_then_delete_clears_both() {
        // the crawl → first-try resolve → delete path. enqueue writes pi|+pq|;
        // the delete (via the *loaded* pending, which carries the matching due)
        // removes both. this is the churn bug: a first-try resolution used to
        // orphan the pq| because attempt-0 had no queue entry to delete.
        let eng = MemEngine::new();
        let did = plc_did("a");

        let mut b = eng.batch();
        PendingIdentity::enqueue_new(did.clone(), None, t(1_000), &mut b);
        b.commit().unwrap();
        assert!(
            PendingIdentity::load(&eng, &did).unwrap().is_some(),
            "pi| written by enqueue_new"
        );
        assert_eq!(
            pq_dids(&eng),
            vec![did.clone()],
            "pq| written by enqueue_new"
        );

        let loaded = PendingIdentity::load(&eng, &did).unwrap().expect("pi|");
        let mut b = eng.batch();
        loaded.delete(&mut b);
        b.commit().unwrap();

        assert!(
            PendingIdentity::load(&eng, &did).unwrap().is_none(),
            "pi| gone"
        );
        assert!(pq_dids(&eng).is_empty(), "pq| gone, not orphaned");
    }

    #[test]
    fn reconcile_delete_clears_pi_and_pq() {
        // the scheduler holds the queue entry (did + due) for an already-resolved
        // repo and reconciles it away — both keys gone.
        let eng = MemEngine::new();
        let did = plc_did("a");
        let mut b = eng.batch();
        PendingIdentity::enqueue_new(did.clone(), None, t(1_000), &mut b);
        b.commit().unwrap();

        let entry = PendingIdentityQueueEntry {
            did: did.clone(),
            due: t(1_000),
        };
        let mut b = eng.batch();
        PendingIdentity::reconcile_delete(entry, &mut b);
        b.commit().unwrap();

        assert!(PendingIdentity::load(&eng, &did).unwrap().is_none());
        assert!(pq_dids(&eng).is_empty());
    }

    #[test]
    fn reconcile_delete_drains_legacy_pq_only_entry() {
        // the existing-backlog shape: a bare pq| entry written by the old
        // request_sync with no pi| behind it. reconcile_delete drains it from
        // the held queue entry alone (the pi| delete is a harmless no-op).
        let eng = MemEngine::new();
        let did = plc_did("legacy");
        let mut b = eng.batch();
        PendingIdentityQueueEntry {
            did: did.clone(),
            due: t(1_000),
        }
        .insert(&mut b);
        b.commit().unwrap();
        assert_eq!(pq_dids(&eng), vec![did.clone()]);

        let entry = PendingIdentityQueueEntry {
            did: did.clone(),
            due: t(1_000),
        };
        let mut b = eng.batch();
        PendingIdentity::reconcile_delete(entry, &mut b);
        b.commit().unwrap();

        assert!(pq_dids(&eng).is_empty(), "legacy pq|-only entry drained");
    }

    #[test]
    fn store_failed_roundtrip_preserves_all_fields() {
        let eng = MemEngine::new();
        let mut p = PendingIdentity::new(plc_did("a"), t(1_000));
        let mut b = eng.batch();
        p.store_failed("err with detail", t(1_000), &mut b);
        b.commit().unwrap();

        let loaded = PendingIdentity::load(&eng, &plc_did("a"))
            .unwrap()
            .expect("present");
        assert_eq!(loaded.did, p.did);
        assert_eq!(loaded.first_seen, p.first_seen);
        assert_eq!(loaded.next_try_at, p.next_try_at);
        assert_eq!(loaded.attempts, p.attempts);
        assert_eq!(loaded.last_error, p.last_error);
    }
}
