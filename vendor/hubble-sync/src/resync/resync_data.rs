//! the default resync data type and machinery
//!
//! apps with specialized resync mechanisms can implement `Resyncable` and to
//! define their own fetch method, data type, and (optionally) measurements.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use metrics::histogram;
use repo_stream::{DiskBuilder, DiskDriver, DriverBuilder, LoadError, MemCar, PartialCar};
use tokio::io::AsyncRead;
use tokio::sync::OwnedSemaphorePermit;

use super::{BigRepoPermits, RepoWalk, ResyncError, Resyncable, TransientResyncError};
use crate::commit::CommitObject;
use crate::metrics::BIG_REPO_WAIT_SECONDS;

const MEM_LIMIT_SMALL_MB: usize = 16; // TODO: wire in from config
const MEM_LIMIT_LARGE_MB: usize = 150;
const SPILL_CACHE_MB: usize = 16;
const SPILL_MAX_STORED_MB: usize = 10 * 1024;

/// just a concurrency-safe unique-spill-directory-name helper (not a metric)
static SPILL_COUNTER: AtomicU64 = AtomicU64::new(0);

#[expect(clippy::large_enum_variant, reason = "mem is bigger and the hot path")]
pub enum LoadedRepo {
    Mem(MemCar),
    Disk(DiskDriver),
}

/// data for the default resync implementation
pub struct ResyncData {
    pub commit: CommitObject,
    pub walk: RepoWalk,
    #[expect(dead_code, reason = "permit released on data drop")]
    big_permit: Option<OwnedSemaphorePermit>,
}

impl ResyncData {
    /// total size of all CAR blocks
    ///
    /// known before walk, but includes any out-of-tree blocks
    pub fn total_block_bytes(&self) -> i64 {
        self.walk.total_block_bytes()
    }
}

impl Resyncable for ResyncData {
    fn commit(&self) -> &CommitObject {
        &self.commit
    }
    fn size(&self) -> Option<i64> {
        Some(self.walk.block_bytes())
    }
    fn count(&self) -> Option<i32> {
        Some(self.walk.records_emitted())
    }
    /// estimate whether this resync wil be over the big-repo threshold
    fn is_big(
        prev_size: Option<i64>,
        prev_count: Option<i32>,
        _commits_since: u32,
        records_delta: i32,
    ) -> bool {
        let Some(prev_count) = prev_count else {
            return false;
        };
        let Some(prev_size) = prev_size else {
            return false;
        };
        if prev_count == 0 {
            // guard against (a ~harmless) division-by-zero
            return prev_size < MEM_LIMIT_SMALL_MB as i64 * 2_i64.pow(20);
        }
        let size_per_count = prev_size as f64 / prev_count as f64;
        let estimate_size_change = records_delta as f64 * size_per_count;
        let estimated_size = prev_size as f64 + estimate_size_change;
        let mb = 2_u32.pow(20) as f64;
        let estimated_size_mb = (estimated_size / mb).ceil() as usize;
        estimated_size_mb >= MEM_LIMIT_SMALL_MB
    }
}

async fn sneak_under<R: AsyncRead + Send + Unpin>(
    reader: R,
) -> Result<Result<MemCar, PartialCar<R>>, LoadError<R>> {
    match DriverBuilder::new()
        .with_mem_limit_mb(MEM_LIMIT_SMALL_MB)
        .load_car(reader)
        .await
    {
        Ok(mem) => Ok(Ok(mem)),
        Err(LoadError::MemoryLimitReached(partial)) => Ok(Err(*partial)),
        Err(e) => Err(e),
    }
}

/// load a getRepo streaming response into repo-stream
///
/// caller should apply cancellation
pub async fn load_repo<R: AsyncRead + Send + Unpin>(
    reader: R,
    big_permits: &BigRepoPermits,
    advance_permit: Option<OwnedSemaphorePermit>,
    reactive_permit_wait: Duration,
    spill_dir: &Path,
) -> Result<ResyncData, ResyncError> {
    let (permit, partial) = if let Some(permit) = advance_permit {
        // we already have a permit, so go ahead with full mem limit
        match DriverBuilder::new()
            .with_mem_limit_mb(MEM_LIMIT_LARGE_MB)
            .load_car(reader)
            .await
        {
            Ok(mem) => {
                let commit = CommitObject::try_from(&mem.commit)
                    .map_err(|e| TransientResyncError::Data(e.to_string()))?;
                return Ok(ResyncData {
                    commit,
                    walk: RepoWalk::new(LoadedRepo::Mem(mem)),
                    big_permit: Some(permit),
                });
            }
            Err(LoadError::MemoryLimitReached(partial)) => (permit, *partial),
            Err(e) => return Err(TransientResyncError::Data(format!("failed to load: {e}")).into()),
        }
    } else {
        // we don't have a permit: try with a small limit, else get a permit
        match sneak_under(reader).await {
            Ok(Ok(mem)) => {
                return Ok(ResyncData {
                    commit: CommitObject::try_from(&mem.commit)
                        .map_err(|e| TransientResyncError::Data(e.to_string()))?,
                    walk: RepoWalk::new(LoadedRepo::Mem(mem)),
                    big_permit: None,
                });
            }
            Ok(Err(partial)) => {
                let wait_start = Instant::now();
                let permit =
                    tokio::time::timeout(reactive_permit_wait, big_permits.acquire_owned())
                        .await
                        .map_err(|_elapsed| ResyncError::ReactivePermitTimeout)?;
                histogram!(BIG_REPO_WAIT_SECONDS, "path" => "reactive")
                    .record(wait_start.elapsed().as_secs_f64());

                match partial.continue_loading(MEM_LIMIT_LARGE_MB).await {
                    Ok(mem) => {
                        return Ok(ResyncData {
                            commit: CommitObject::try_from(&mem.commit)
                                .map_err(|e| TransientResyncError::Data(e.to_string()))?,
                            walk: RepoWalk::new(LoadedRepo::Mem(mem)),
                            big_permit: Some(permit),
                        });
                    }
                    Err(LoadError::MemoryLimitReached(partial)) => (permit, *partial),
                    Err(e) => {
                        return Err(
                            TransientResyncError::Data(format!("failed to load: {e}")).into()
                        );
                    }
                }
            }
            Err(e) => return Err(TransientResyncError::Data(format!("failed to load: {e}")).into()),
        }
    };

    // we have a partial repo after full in-memory limit loads (both paths)
    // so it's time to spill to disk
    let n = SPILL_COUNTER.fetch_add(1, Ordering::Relaxed);

    let store = DiskBuilder::new()
        .with_cache_size_mb(SPILL_CACHE_MB)
        .with_max_stored_mb(SPILL_MAX_STORED_MB)
        .open(spill_dir.join(format!("resync-{n}")))
        .await
        .map_err(|e| TransientResyncError::Data(format!("failed to open for disk spill: {e}")))?;

    let (rs_commit, _, driver) = partial
        .finish_loading(store)
        .await
        .map_err(|e| TransientResyncError::Data(format!("failed to load via disk spill: {e}")))?;

    Ok(ResyncData {
        commit: CommitObject::try_from(&rs_commit)
            .map_err(|e| TransientResyncError::Data(e.to_string()))?,
        walk: RepoWalk::new(LoadedRepo::Disk(driver)),
        big_permit: Some(permit),
    })
}
