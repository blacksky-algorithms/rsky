//! Per-actor advisory file locks under a shared directory.
//!
//! Every write on an actor holds its lock shared; a maintenance drain takes
//! it exclusively, which waits for in-flight writes to finish and keeps new
//! ones from starting until it is released. The locks are `flock` locks, so
//! they are visible across processes sharing the directory and vanish with
//! the holder if it dies.

use anyhow::{bail, Context, Result};
use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct LockDir {
    directory: PathBuf,
}

/// A held lock; dropping it releases the lock.
#[derive(Debug)]
pub struct FileLock {
    _file: File,
    pub path: PathBuf,
    pub exclusive: bool,
}

fn flock(file: &File, operation: libc::c_int) -> std::io::Result<()> {
    // SAFETY: flock only reads the descriptor number and the operation flags.
    let rc = unsafe { libc::flock(file.as_raw_fd(), operation) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

impl LockDir {
    pub fn new(directory: impl AsRef<Path>) -> Result<Self> {
        let directory = directory.as_ref().to_path_buf();
        std::fs::create_dir_all(&directory)
            .with_context(|| format!("cannot create lock directory {}", directory.display()))?;
        Ok(LockDir { directory })
    }

    pub fn path_for(&self, did: &str) -> Result<PathBuf> {
        if did.is_empty() || did.contains('/') || did.contains('\\') || did.starts_with('.') {
            bail!("unsafe lock name: {did}");
        }
        Ok(self.directory.join(format!("{did}.lock")))
    }

    fn open(&self, did: &str) -> Result<(File, PathBuf)> {
        let path = self.path_for(did)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("cannot open lock {}", path.display()))?;
        Ok((file, path))
    }

    /// Takes the actor's lock shared, waiting for an exclusive holder to
    /// release it.
    pub async fn shared(&self, did: &str) -> Result<FileLock> {
        let (file, path) = self.open(did)?;
        tokio::task::spawn_blocking(move || {
            flock(&file, libc::LOCK_SH)?;
            Ok(FileLock {
                _file: file,
                path,
                exclusive: false,
            })
        })
        .await?
    }

    /// Takes the actor's lock exclusively without waiting; `None` when a
    /// holder is in the way.
    pub fn try_exclusive(&self, did: &str) -> Result<Option<FileLock>> {
        let (file, path) = self.open(did)?;
        match flock(&file, libc::LOCK_EX | libc::LOCK_NB) {
            Ok(()) => Ok(Some(FileLock {
                _file: file,
                path,
                exclusive: true,
            })),
            Err(err) => (err.kind() == std::io::ErrorKind::WouldBlock)
                .then_some(None)
                .ok_or_else(|| anyhow::Error::from(err)),
        }
    }

    /// Takes the actor's lock exclusively, retrying until `timeout` passes.
    pub async fn exclusive_within(&self, did: &str, timeout: Duration) -> Result<FileLock> {
        let started = Instant::now();
        loop {
            if let Some(lock) = self.try_exclusive(did)? {
                return Ok(lock);
            }
            if started.elapsed() >= timeout {
                bail!("{did} still has writes in flight after {timeout:?}");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn shared_holders_coexist_and_block_an_exclusive_taker() {
        let dir = tempfile::tempdir().unwrap();
        let locks = LockDir::new(dir.path().join("locks")).unwrap();
        let one = locks.shared("did:plc:a").await.unwrap();
        let two = locks.shared("did:plc:a").await.unwrap();
        assert!(!one.exclusive);
        assert!(one.path.ends_with("did:plc:a.lock"));
        assert!(locks.try_exclusive("did:plc:a").unwrap().is_none());
        let err = locks
            .exclusive_within("did:plc:a", Duration::from_millis(60))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("in flight"));
        // another actor is unaffected
        assert!(locks.try_exclusive("did:plc:b").unwrap().is_some());
        drop(one);
        assert!(locks.try_exclusive("did:plc:a").unwrap().is_none());
        drop(two);
        let exclusive = locks
            .exclusive_within("did:plc:a", Duration::from_secs(1))
            .await
            .unwrap();
        assert!(exclusive.exclusive);
        drop(exclusive);
        assert!(locks.try_exclusive("did:plc:a").unwrap().is_some());
    }

    #[tokio::test]
    async fn an_exclusive_holder_delays_a_shared_taker() {
        let dir = tempfile::tempdir().unwrap();
        let locks = LockDir::new(dir.path().join("locks")).unwrap();
        let exclusive = locks.try_exclusive("did:plc:a").unwrap().unwrap();
        let waiter = {
            let locks = locks.clone();
            tokio::spawn(async move { locks.shared("did:plc:a").await.map(|lock| lock.exclusive) })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!waiter.is_finished());
        drop(exclusive);
        assert!(!waiter.await.unwrap().unwrap());
    }

    #[test]
    fn rejects_unsafe_names_and_unusable_directories() {
        let dir = tempfile::tempdir().unwrap();
        let locks = LockDir::new(dir.path().join("locks")).unwrap();
        for name in ["", "a/b", "a\\b", ".hidden"] {
            assert!(locks.path_for(name).is_err());
            assert!(locks.try_exclusive(name).is_err());
        }
        let file = dir.path().join("file");
        std::fs::write(&file, b"x").unwrap();
        assert!(LockDir::new(file.join("locks")).is_err());
    }
}
