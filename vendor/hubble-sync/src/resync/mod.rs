mod big_repo_permits;
mod repo_walk;
mod resync_data;

pub use big_repo_permits::BigRepoPermits;
pub use resync_data::{ResyncData, load_repo};

use crate::{AccountStatus, CommitObject, Did};

use repo_walk::RepoWalk;
use resync_data::LoadedRepo;

#[derive(Debug, thiserror::Error)]
pub enum ResyncError {
    #[error("resync was cancelled before completing")]
    Cancelled,
    #[error("timed out waiting for a big-mem resync permit")]
    ReactivePermitTimeout,
    #[error("no permit avaialble to pre-acquire")]
    NoPermitAvailable,
    #[error("transient failure: {0}")]
    Transient(#[from] TransientResyncError),
    #[error("wrong DID, expected {expected}, found {got:?}")]
    WrongDid { expected: Did, got: Did },
    #[error("non-active account with status: {0:?}")]
    Status(AccountStatus),
    #[error("repo not found on host")]
    RepoMissing,
}

#[derive(Debug, thiserror::Error)]
pub enum TransientResyncError {
    #[error("request error: {0}")]
    Request(String),
    #[error("timeout after {0:?}")]
    Timeout(#[from] tokio::time::error::Elapsed),
    #[error("any other resync data error: {0}")]
    Data(String),
}

/// The interface for custom resync data types
///
/// At minumum, [`commit`] must be implemented, so that the resync can be
/// verified.
///
/// [`size`] and [`count`] are called after the consumer app's `apply_resync`
/// method returns. Non-`None` values will be recorded with the last-resync-
/// context for that user The meaning of "size" and "count" are up to the
/// resync implementation.
///
/// [`is_big`] receives the previously recorded `size` and `count` values (if
/// reported) plus counts of commits and total change in record count since the
/// last resync. If `true`, a `big_repo` permit will be _pre-acquired_ before
/// calling ConsumerApp::resync.
///
/// For example: the default resync data reports the total MST block bytes for
/// `size`, and the count of records emitted via `next_chunk()` during
/// apply_resync for `count`. In future resyncs, it compares its last `size` to
/// a threshold to request that a permit be pre-acquired.
pub trait Resyncable: Send {
    /// required, verifies the resynd (valid rev, correct-DID, signature check)
    fn commit(&self) -> &CommitObject;
    /// optional, means whatever you want it to mean
    fn size(&self) -> Option<i64> {
        None
    }
    /// optional, means whatever you want it to mean
    fn count(&self) -> Option<i32> {
        None
    }
    /// optional, "big-repo" permit pre-acquired if true
    #[allow(unused_variables, reason = "trait implementers may use")]
    fn is_big(
        size_at_last_resync: Option<i64>,
        count_at_last_resync: Option<i32>,
        commits_since_last_resync: u32,
        records_delta_since_last_resync: i32,
    ) -> bool {
        false
    }
}
