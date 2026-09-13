//! A request body spooled to disk so that a large upload never has to fit
//! in memory. The file is removed when the handle is dropped, so a failed
//! or abandoned upload leaves nothing behind.

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static SPOOL_IDS: AtomicU64 = AtomicU64::new(0);

pub struct SpoolFile {
    path: PathBuf,
}

impl SpoolFile {
    /// Reserves a new file name under `dir`, creating the directory.
    pub async fn new(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        tokio::fs::create_dir_all(dir).await?;
        let path = dir.join(format!(
            "upload-{}-{}",
            std::process::id(),
            SPOOL_IDS.fetch_add(1, Ordering::Relaxed)
        ));
        Ok(SpoolFile { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for SpoolFile {
    fn drop(&mut self) {
        if let Err(err) = std::fs::remove_file(&self.path) {
            if err.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(?err, path = %self.path.display(), "spool file not removed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_file_goes_away_with_its_handle() {
        let dir = tempfile::tempdir().unwrap();
        let spool = SpoolFile::new(dir.path().join("spool")).await.unwrap();
        tokio::fs::write(spool.path(), b"payload").await.unwrap();
        let path = spool.path().to_path_buf();
        assert!(path.exists());
        drop(spool);
        assert!(!path.exists());
        // an already-removed file is not an error
        let spool = SpoolFile::new(dir.path().join("spool")).await.unwrap();
        assert!(!spool.path().exists());
        drop(spool);
        // a file that cannot be removed is reported, not fatal
        let locked = SpoolFile::new(dir.path().join("locked")).await.unwrap();
        tokio::fs::create_dir_all(locked.path()).await.unwrap();
        drop(locked);
    }
}
