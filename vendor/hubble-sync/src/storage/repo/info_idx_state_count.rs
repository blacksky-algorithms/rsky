//! Aggregate index of repo counts by sync state
//!
//! see `storage/repo/mod.rs` for the nice entries into this
//!
//! Pending repos (seen-DIDs that we couldn't resolve an identity for) are not
//! counted.
//!
//! "rc|"||<status> => count:i64_be
//!
//! where `status` is a `SyncStatusKind` name, except `desynchronized`, which is
//! broken out by why the repo desynced:
//!
//! "rc|desynchronized|"||<reason> => count:i64_be
//!
//! where `reason` is a `DesyncReasonKind` name.

use std::collections::BTreeMap;

use crate::{LoadError, StorageBatch, StorageEngine, StorageError};

use super::{DesyncReasonKind, PREFIX_REPO_INFO_IDX_COUNT, SyncStatus, SyncStatusKind};

/// a counted bucket: sync states + `desynchronized` by reason
#[derive(Debug, Clone, PartialEq)]
enum Bucket {
    State(SyncStatusKind), // synchronized | deactivated | gone
    Desync(DesyncReasonKind),
}

impl Bucket {
    fn of(status: &SyncStatus) -> Self {
        match status {
            SyncStatus::Desynchronized(d) => Bucket::Desync(d.reason.kind()),
            other => Bucket::State(other.kind()),
        }
    }

    fn encode_key(&self) -> Vec<u8> {
        match self {
            Bucket::State(k) => [PREFIX_REPO_INFO_IDX_COUNT, k.name().as_bytes()].concat(),
            Bucket::Desync(r) => [
                PREFIX_REPO_INFO_IDX_COUNT.as_slice(),
                b"desynchronized|",
                r.name().as_bytes(),
            ]
            .concat(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Index {
    Should(Bucket),
    ShouldNot,
}

impl Index {
    fn should(stat: Option<&SyncStatus>) -> Self {
        stat.map(|s| Index::Should(Bucket::of(s)))
            .unwrap_or(Index::ShouldNot)
    }
}

impl SyncStatus {
    pub(super) fn reconcile_for<E: StorageError, B: StorageBatch<E>>(
        prev: Option<&SyncStatus>,
        next: Option<&SyncStatus>,
        batch: &mut B,
    ) {
        let should_prev = Index::should(prev);
        let should_next = Index::should(next);

        // no change to index: short circuit
        if should_prev == should_next {
            return;
        }

        // change from previous: decrement prev
        if let Index::Should(p) = should_prev {
            batch.increment_counter(&p.encode_key(), -1);
        }

        // into next: increment
        if let Index::Should(n) = should_next {
            batch.increment_counter(&n.encode_key(), 1);
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RepoCountsByState {
    pub synchronized: u64,
    pub out_of_scope: u64,
    /// total across every desync reason
    pub desynchronized: u64,
    pub non_active: u64,
    pub gone: u64,
    /// desynced repos by the reason they desynced
    pub desynchronized_by_reason: BTreeMap<&'static str, u64>,
}

impl RepoCountsByState {
    fn read<S: StorageEngine>(
        storage: &S,
        key: &[u8],
        label: &str,
    ) -> Result<u64, LoadError<S::Error>> {
        let raw = storage.get_counter(key).map_err(LoadError::Storage)?;
        raw.try_into() // i64 -> u64
            .map_err(|_| LoadError::Integrity(format!("negative {label} repo count: {raw}")))
    }

    pub fn load_state<S: StorageEngine>(
        storage: &S,
        kind: SyncStatusKind,
    ) -> Result<u64, LoadError<S::Error>> {
        match kind {
            SyncStatusKind::Desynchronized => {
                let mut total = 0;
                for &r in DesyncReasonKind::ALL {
                    total += Self::read(storage, &Bucket::Desync(r).encode_key(), r.name())?;
                }
                Ok(total)
            }
            other => Self::read(storage, &Bucket::State(other).encode_key(), other.name()),
        }
    }

    pub fn load<S: StorageEngine>(storage: &S) -> Result<Self, LoadError<S::Error>> {
        // TODO: wow we really are missing snapshots for reads
        let mut desynchronized_by_reason = BTreeMap::new();
        let mut desynchronized = 0u64;
        for &r in DesyncReasonKind::ALL {
            let n = Self::read(storage, &Bucket::Desync(r).encode_key(), r.name())?;
            desynchronized += n;
            desynchronized_by_reason.insert(r.name(), n);
        }
        Ok(Self {
            synchronized: Self::load_state(storage, SyncStatusKind::Synchronized)?,
            out_of_scope: Self::load_state(storage, SyncStatusKind::OutOfScope)?,
            desynchronized,
            non_active: Self::load_state(storage, SyncStatusKind::NonActive)?,
            gone: Self::load_state(storage, SyncStatusKind::Gone)?,
            desynchronized_by_reason,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StorageEngine;
    use crate::storage::engine::mem::MemEngine;
    use crate::storage::repo::{DesyncReason, Desynchronized};
    use std::time::SystemTime;

    fn desync(reason: DesyncReason) -> SyncStatus {
        SyncStatus::Desynchronized(Desynchronized::new(reason, SystemTime::now(), 0, None))
    }

    fn count(eng: &MemEngine, suffix: &[u8]) -> i64 {
        eng.get_counter(&[PREFIX_REPO_INFO_IDX_COUNT, suffix].concat())
            .unwrap()
    }

    fn reconcile(eng: &MemEngine, prev: Option<&SyncStatus>, next: Option<&SyncStatus>) {
        let mut b = eng.batch();
        SyncStatus::reconcile_for(prev, next, &mut b);
        b.commit().unwrap();
    }

    #[test]
    fn into_a_bucket_increments() {
        let eng = MemEngine::new();
        reconcile(&eng, None, Some(&SyncStatus::Synchronized));
        assert_eq!(count(&eng, b"synchronized"), 1);
    }

    #[test]
    fn out_of_a_bucket_decrements() {
        let eng = MemEngine::new();
        reconcile(&eng, None, Some(&SyncStatus::Synchronized));
        reconcile(&eng, Some(&SyncStatus::Synchronized), None);
        assert_eq!(count(&eng, b"synchronized"), 0);
    }

    #[test]
    fn between_buckets_moves_one() {
        let eng = MemEngine::new();
        reconcile(&eng, None, Some(&SyncStatus::Synchronized));
        reconcile(
            &eng,
            Some(&SyncStatus::Synchronized),
            Some(&SyncStatus::NonActive {
                since: SystemTime::now(),
                rev: None,
            }),
        );
        assert_eq!(count(&eng, b"synchronized"), 0);
        assert_eq!(count(&eng, b"nonActive"), 1);
    }

    #[test]
    fn same_bucket_is_a_noop() {
        let eng = MemEngine::new();
        reconcile(&eng, None, Some(&SyncStatus::Synchronized)); // synchronized = 1
        reconcile(
            &eng,
            Some(&SyncStatus::Synchronized),
            Some(&SyncStatus::Synchronized),
        );
        assert_eq!(count(&eng, b"synchronized"), 1, "same-variant is a no-op");
    }

    #[test]
    fn load_reads_every_bucket() {
        let eng = MemEngine::new();
        reconcile(&eng, None, Some(&SyncStatus::Synchronized));
        reconcile(&eng, None, Some(&SyncStatus::Synchronized));
        reconcile(
            &eng,
            None,
            Some(&SyncStatus::NonActive {
                since: SystemTime::now(),
                rev: None,
            }),
        );
        let counts = RepoCountsByState::load(&eng).unwrap();
        assert_eq!(counts.synchronized, 2);
        assert_eq!(counts.non_active, 1);
        assert_eq!(counts.desynchronized, 0);
        assert_eq!(counts.gone, 0);
    }

    #[test]
    fn load_state_reads_a_single_bucket() {
        let eng = MemEngine::new();
        reconcile(&eng, None, Some(&SyncStatus::Synchronized));
        assert_eq!(
            RepoCountsByState::load_state(&eng, SyncStatusKind::Synchronized).unwrap(),
            1
        );
        assert_eq!(
            RepoCountsByState::load_state(&eng, SyncStatusKind::Gone).unwrap(),
            0
        );
    }

    #[test]
    fn load_state_rejects_a_negative_count() {
        // a count should never go negative; if one does, surface it, don't wrap.
        let eng = MemEngine::new();
        reconcile(&eng, Some(&SyncStatus::Synchronized), None); // 0 -> -1
        let err = RepoCountsByState::load_state(&eng, SyncStatusKind::Synchronized).unwrap_err();
        assert!(matches!(err, LoadError::Integrity(_)));
    }

    #[test]
    fn changing_desync_reason_moves_between_reason_buckets() {
        // desync -> desync with a *different* reason is not a no-op: it moves one
        // from the old reason bucket to the new one.
        let eng = MemEngine::new();
        let first = desync(DesyncReason::FirstSeen);
        let fail = desync(DesyncReason::FirehoseFail {
            bad_commit_rev: None,
        });
        reconcile(&eng, None, Some(&first));
        assert_eq!(count(&eng, b"desynchronized|first_seen"), 1);
        reconcile(&eng, Some(&first), Some(&fail));
        assert_eq!(count(&eng, b"desynchronized|first_seen"), 0);
        assert_eq!(count(&eng, b"desynchronized|firehose_fail"), 1);
    }

    #[test]
    fn load_breaks_out_desync_by_reason() {
        let eng = MemEngine::new();
        reconcile(&eng, None, Some(&desync(DesyncReason::FirstSeen)));
        reconcile(&eng, None, Some(&desync(DesyncReason::FirstSeen)));
        reconcile(&eng, None, Some(&desync(DesyncReason::Sync11Lax)));

        let counts = RepoCountsByState::load(&eng).unwrap();
        assert_eq!(counts.desynchronized, 3, "total sums across reasons");
        assert_eq!(counts.desynchronized_by_reason.get("first_seen"), Some(&2));
        assert_eq!(counts.desynchronized_by_reason.get("sync11_lax"), Some(&1));
        assert_eq!(
            counts.desynchronized_by_reason.get("throttled"),
            Some(&0),
            "every reason is present, even at zero"
        );
        // load_state sums desync across reasons the same way
        assert_eq!(
            RepoCountsByState::load_state(&eng, SyncStatusKind::Desynchronized).unwrap(),
            3
        );
    }
}
