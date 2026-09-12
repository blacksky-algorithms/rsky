//! The write-ahead mutation journal: one line appended and synced before
//! the first byte of a mutation goes upstream, one more after the upstream
//! answered. The audit reads these files by byte offset; a failure to
//! record is a failure to forward.

use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Debug, Serialize)]
struct Start<'a> {
    t: String,
    id: u64,
    phase: &'static str,
    nsid: &'a str,
    did: Option<&'a str>,
    backend: &'a str,
}

#[derive(Debug, Serialize)]
struct End {
    t: String,
    id: u64,
    phase: &'static str,
    status: u16,
}

/// Where journal lines go: appended, then made durable.
pub trait Sink: Write + Send {
    fn sync(&mut self) -> std::io::Result<()>;
}

impl Sink for File {
    fn sync(&mut self) -> std::io::Result<()> {
        self.sync_data()
    }
}

pub struct Journal {
    path: PathBuf,
    file: Mutex<Box<dyn Sink>>,
    next_id: AtomicU64,
}

impl Journal {
    /// Opens the journal for appending, creating it.
    pub fn open(path: &Path) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let next_id = file.metadata()?.len();
        Ok(Self::over(path, file, next_id))
    }

    /// Wraps a caller-supplied sink; ids start at `next_id`.
    pub fn over(path: &Path, sink: impl Sink + 'static, next_id: u64) -> Self {
        Self {
            path: path.to_path_buf(),
            file: Mutex::new(Box::new(sink)),
            next_id: AtomicU64::new(next_id),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn append(&self, line: &str) -> std::io::Result<()> {
        let mut file = self.file.lock().expect("journal poisoned");
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync()
    }

    /// Records that a mutation is about to be forwarded; the id names its
    /// end line.
    pub fn start(&self, nsid: &str, did: Option<&str>, backend: &str) -> std::io::Result<u64> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let line = serde_json::to_string(&Start {
            t: now(),
            id,
            phase: "start",
            nsid,
            did,
            backend,
        })
        .expect("journal line serializes");
        self.append(&line)?;
        Ok(id)
    }

    /// Records the upstream's answer to a started mutation.
    pub fn end(&self, id: u64, status: u16) -> std::io::Result<()> {
        let line = serde_json::to_string(&End {
            t: now(),
            id,
            phase: "end",
            status,
        })
        .expect("journal line serializes");
        self.append(&line)
    }
}

fn now() -> String {
    let since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!(
        "{}.{:03}",
        since_epoch.as_secs(),
        since_epoch.subsec_millis()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_appended_in_order_with_matching_ids() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("router").join("mutations.jsonl");
        let journal = Journal::open(&path).unwrap();
        assert_eq!(journal.path(), path);
        let id = journal
            .start("com.atproto.repo.createRecord", Some("did:plc:a"), "ts")
            .unwrap();
        journal.end(id, 200).unwrap();
        let other = journal
            .start("com.atproto.server.createInviteCode", None, "ts")
            .unwrap();
        assert_eq!(other, id + 1);
        let lines: Vec<serde_json::Value> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["phase"], "start");
        assert_eq!(lines[0]["did"], "did:plc:a");
        assert_eq!(lines[0]["backend"], "ts");
        assert_eq!(lines[1]["phase"], "end");
        assert_eq!(lines[1]["id"], lines[0]["id"]);
        assert_eq!(lines[1]["status"], 200);
        assert!(lines[2]["did"].is_null());
        // ids continue past the existing file on reopen
        let reopened = Journal::open(&path).unwrap();
        let next = reopened.start("x", None, "rsky").unwrap();
        assert!(next > other);
        // an unwritable location is an error, not a silent skip
        assert!(Journal::open(&dir.path().join("router")).is_err());
        assert!(Journal::open(&path.join("below-a-file")).is_err());
    }
}
