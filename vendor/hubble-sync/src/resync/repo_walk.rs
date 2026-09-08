//! repo-stream wrapper for default resyncdata

use std::sync::Arc;

use repo_stream::walk::Output as MemRecord;

use crate::resync::LoadedRepo;
use crate::{ConsumerAppError, StorageError};

#[derive(Debug, thiserror::Error)]
pub enum RepoWalkError {
    #[error("mem walk: {0}")]
    Mem(#[from] repo_stream::walk::WalkError),
    #[error("disk walk: {0}")]
    Disk(#[from] repo_stream::disk::DriveError),
}

impl<E: StorageError> From<RepoWalkError> for ConsumerAppError<E> {
    /// auto-convert the default ResyncData's error into a repo desync
    ///
    /// (slightly opinionated deafult)
    fn from(we: RepoWalkError) -> ConsumerAppError<E> {
        ConsumerAppError::Desynchronize {
            reason: we.to_string(),
        }
    }
}

/// one record from the repo export
///
/// CID omitted for now bc if forgot to put it on the repo-stream disk driver
pub struct WalkRecord {
    /// the "repo path", the full raw "<collection>/<rkey>"
    pub key: Arc<str>,
    /// the record block (raw cbor bytes)
    pub data: Arc<[u8]>,
}

impl From<MemRecord> for WalkRecord {
    fn from(mr: MemRecord) -> Self {
        Self {
            key: mr.key.into(),
            data: mr.data.into(),
        }
    }
}

/// repo-stream wrapper that keeps some stats and simplifies the interface
///
/// use `.get_mut()` if you need the raw repo-stream apis (stats will not be
/// tracked).
pub struct RepoWalk {
    inner: LoadedRepo,
    records: i32,
    bytes: i64,
}

impl RepoWalk {
    pub(crate) fn new(inner: LoadedRepo) -> Self {
        Self {
            inner,
            records: 0,
            bytes: 0,
        }
    }

    pub fn inner_mut(&mut self) -> &mut LoadedRepo {
        &mut self.inner
    }

    pub fn next_chunk(&mut self, n: usize) -> Result<Option<Vec<WalkRecord>>, RepoWalkError> {
        let out: Vec<WalkRecord> = match &mut self.inner {
            LoadedRepo::Mem(car) => {
                let Some(items) = car.next_chunk_strict(n)? else {
                    return Ok(None);
                };
                items.into_iter().map(WalkRecord::from).collect()
            }
            LoadedRepo::Disk(driver) => {
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    let Some((k, b)) = driver.next_blocking()? else {
                        break;
                    };
                    v.push(WalkRecord {
                        key: k.into(),
                        data: b.into(),
                    });
                }
                if v.is_empty() {
                    return Ok(None);
                }
                v
            }
        };
        self.bytes += out.iter().map(|r| r.data.len() as i64).sum::<i64>();
        self.records += out.len() as i32;
        Ok(Some(out))
    }

    /// pre-walk total block bytes size: includes any out-of-tree blocks
    pub fn total_block_bytes(&self) -> i64 {
        match &self.inner {
            LoadedRepo::Mem(car) => car.loaded_bytes() as i64,
            LoadedRepo::Disk(driver) => driver.loaded_bytes() as i64,
        }
    }

    /// post-walk total block bytes size: excludes out-of-tree blocks
    pub fn block_bytes(&self) -> i64 {
        self.bytes
    }

    pub fn records_emitted(&self) -> i32 {
        self.records
    }
}
