//! Aggregate index of repo counts by (effective) account status
//!
//! see `storage/repo/mod.rs` for the nice entries into this.
//!
//! effective status: locally-applied moderation is applied for these counts,
//! not just the upstream value.
//!
//! "ac|"||<Status> => count:i64_be
//!
//! where `Status` is from `AccountStatusKind` names

use crate::{LoadError, StorageBatch, StorageEngine, StorageError};

use super::{AccountStatus, AccountStatusKind, PREFIX_REPO_INFO_IDX_ACCOUNT_COUNT};

fn encode_key(kind: AccountStatusKind) -> Vec<u8> {
    [PREFIX_REPO_INFO_IDX_ACCOUNT_COUNT, kind.name().as_bytes()].concat()
}

impl AccountStatus {
    /// `prev`/`next` are the effective account statuses before/after a change
    /// (`None` = not counted, i.e. before a repo exists).
    pub(super) fn reconcile_for<E: StorageError, B: StorageBatch<E>>(
        prev: Option<&AccountStatus>,
        next: Option<&AccountStatus>,
        batch: &mut B,
    ) {
        let prev_kind = prev.map(AccountStatus::kind);
        let next_kind = next.map(AccountStatus::kind);

        // no change to the counted bucket: short circuit
        if prev_kind == next_kind {
            return;
        }
        if let Some(p) = prev_kind {
            batch.increment_counter(&encode_key(p), -1);
        }
        if let Some(n) = next_kind {
            batch.increment_counter(&encode_key(n), 1);
        }
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct RepoCountsByStatus {
    pub active: u64,
    pub deactivated: u64,
    pub suspended: u64,
    pub takendown: u64,
    pub deleted: u64,
    pub other: u64,
}

impl RepoCountsByStatus {
    pub fn load_status<S: StorageEngine>(
        storage: &S,
        kind: AccountStatusKind,
    ) -> Result<u64, LoadError<S::Error>> {
        let raw = storage
            .get_counter(&encode_key(kind))
            .map_err(LoadError::Storage)?;
        raw.try_into() // i64 -> u64
            .map_err(|_| LoadError::Integrity(format!("negative {kind:?} account count: {raw}")))
    }
    pub fn load<S: StorageEngine>(storage: &S) -> Result<Self, LoadError<S::Error>> {
        Ok(Self {
            active: Self::load_status(storage, AccountStatusKind::Active)?,
            deactivated: Self::load_status(storage, AccountStatusKind::Deactivated)?,
            suspended: Self::load_status(storage, AccountStatusKind::Suspended)?,
            takendown: Self::load_status(storage, AccountStatusKind::Takendown)?,
            deleted: Self::load_status(storage, AccountStatusKind::Deleted)?,
            other: Self::load_status(storage, AccountStatusKind::Other)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StorageEngine;
    use crate::storage::engine::mem::MemEngine;

    fn count(eng: &MemEngine, suffix: &[u8]) -> i64 {
        eng.get_counter(&[PREFIX_REPO_INFO_IDX_ACCOUNT_COUNT, suffix].concat())
            .unwrap()
    }

    fn reconcile(eng: &MemEngine, prev: Option<&AccountStatus>, next: Option<&AccountStatus>) {
        let mut b = eng.batch();
        AccountStatus::reconcile_for(prev, next, &mut b);
        b.commit().unwrap();
    }

    #[test]
    fn create_increments_active() {
        let eng = MemEngine::new();
        reconcile(&eng, None, Some(&AccountStatus::Active));
        assert_eq!(count(&eng, b"active"), 1);
    }

    #[test]
    fn transition_moves_one_bucket() {
        let eng = MemEngine::new();
        reconcile(&eng, None, Some(&AccountStatus::Active));
        reconcile(
            &eng,
            Some(&AccountStatus::Active),
            Some(&AccountStatus::Suspended),
        );
        assert_eq!(count(&eng, b"active"), 0);
        assert_eq!(count(&eng, b"suspended"), 1);
    }

    #[test]
    fn masked_transition_is_a_noop() {
        // e.g. upstream Active->Deactivated while locally suspended: effective
        // stays Suspended, so the count must not move.
        let eng = MemEngine::new();
        reconcile(&eng, None, Some(&AccountStatus::Suspended));
        reconcile(
            &eng,
            Some(&AccountStatus::Suspended),
            Some(&AccountStatus::Suspended),
        );
        assert_eq!(count(&eng, b"suspended"), 1);
    }

    #[test]
    fn inactive_variants_bucket_to_other() {
        let eng = MemEngine::new();
        reconcile(
            &eng,
            None,
            Some(&AccountStatus::Inactive("weird".to_string())),
        );
        assert_eq!(count(&eng, b"other"), 1);
    }

    #[test]
    fn load_reads_every_bucket() {
        let eng = MemEngine::new();
        reconcile(&eng, None, Some(&AccountStatus::Active));
        reconcile(&eng, None, Some(&AccountStatus::Active));
        reconcile(&eng, None, Some(&AccountStatus::Takendown));
        let c = RepoCountsByStatus::load(&eng).unwrap();
        assert_eq!(c.active, 2);
        assert_eq!(c.takendown, 1);
        assert_eq!(c.suspended, 0);
    }

    #[test]
    fn load_status_rejects_negative() {
        let eng = MemEngine::new();
        reconcile(&eng, Some(&AccountStatus::Active), None); // 0 -> -1
        let err = RepoCountsByStatus::load_status(&eng, AccountStatusKind::Active).unwrap_err();
        assert!(matches!(err, LoadError::Integrity(_)));
    }
}
