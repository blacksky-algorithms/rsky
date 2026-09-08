use crate::config::{BLOCK_SIZE, CACHE_SIZE, FSYNC_MS, MEMTABLE_SIZE, WRITE_BUFFER_SIZE};

const LIVE_SEQ_READ_CURSOR: &str = "live_seq_read";
const LIVE_LEGACY_DRAINED: &str = "live_legacy_drained";
use crate::types::{FirehoseEvent, IndexJob, WintermuteError};
use fjall::{Config, Keyspace, PartitionCreateOptions, PartitionHandle};
use std::ops::Bound;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Snapshot of the keyspace counters that bear on resident memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyspaceStats {
    /// Bytes currently held in memtables across every partition.
    pub write_buffer_bytes: u64,
    /// Journal files open. Grows when flushes fall behind.
    pub journal_count: u64,
    /// Total on-disk size of the store.
    pub disk_bytes: u64,
}

pub struct Storage {
    #[allow(dead_code)] // Kept for Fjall keyspace - partitions reference it internally
    db: Arc<Keyspace>,
    firehose_events: PartitionHandle,
    firehose_live: PartitionHandle,
    // Sequence-keyed successor to `firehose_live`: monotonic u64 keys mean the
    // read cursor only moves forward and dequeue tombstones are never
    // re-scanned. The legacy uri-keyed partition drains first, then this one.
    firehose_live_seq: PartitionHandle,
    label_live: PartitionHandle,
    cursors: PartitionHandle,
    // Sweep cursor: keys are uri-first, so intake interleaves with the dequeue
    // head and tombstones pile up there; scanning from the last dequeued key
    // (wrapping when exhausted) avoids re-walking them every batch.
    firehose_live_cursor: std::sync::Mutex<Option<Vec<u8>>>,
    firehose_live_seq_cursor: std::sync::Mutex<Option<Vec<u8>>>,
    live_seq_next: AtomicU64,
    legacy_live_drained: AtomicBool,
    live_notify: tokio::sync::Notify,
}

impl Storage {
    pub fn new(db_path: Option<PathBuf>) -> Result<Self, WintermuteError> {
        let path = db_path.unwrap_or_else(|| "backfill_cache".into());

        // Try to open, recover from corruption if needed
        match Self::open_db(&path) {
            Ok(storage) => Ok(storage),
            Err(e) if e.is_storage_corrupted() => {
                tracing::warn!(
                    "detected corrupted storage at {}, deleting and recreating: {e}",
                    path.display()
                );
                crate::metrics::STORAGE_RECOVERY_TOTAL.inc();

                // Delete corrupted database
                if let Err(rm_err) = std::fs::remove_dir_all(&path) {
                    tracing::warn!("failed to remove corrupted db directory: {rm_err}");
                }

                // Retry opening (will create fresh)
                Self::open_db(&path)
            }
            Err(e) => Err(e),
        }
    }

    fn open_db(path: &PathBuf) -> Result<Self, WintermuteError> {
        tracing::info!(
            "opening Fjall with cache={}GB, write_buffer={}GB, memtable={}MB",
            *CACHE_SIZE / (1024 * 1024 * 1024),
            *WRITE_BUFFER_SIZE / (1024 * 1024 * 1024),
            *MEMTABLE_SIZE / (1024 * 1024)
        );
        let db = Config::new(path)
            .cache_size(*CACHE_SIZE)
            .max_write_buffer_size(*WRITE_BUFFER_SIZE)
            .fsync_ms(FSYNC_MS)
            .open()
            .map_err(|e| {
                let err: WintermuteError = e.into();
                if err.is_storage_corrupted() {
                    err
                } else {
                    WintermuteError::Other(format!("failed to open database: {err}"))
                }
            })?;

        let db = Arc::new(db);

        let firehose_events = db.open_partition(
            "firehose_events",
            PartitionCreateOptions::default()
                .max_memtable_size(*MEMTABLE_SIZE)
                .block_size(BLOCK_SIZE),
        )?;

        let firehose_live = db.open_partition(
            "firehose_live",
            PartitionCreateOptions::default()
                .max_memtable_size(*MEMTABLE_SIZE)
                .block_size(BLOCK_SIZE),
        )?;

        let firehose_live_seq = db.open_partition(
            "firehose_live_seq",
            PartitionCreateOptions::default()
                .max_memtable_size(*MEMTABLE_SIZE)
                .block_size(BLOCK_SIZE),
        )?;
        let live_seq_start = firehose_live_seq.last_key_value()?.map_or(0, |(k, _)| {
            k.as_ref()
                .try_into()
                .map_or(0, |b: [u8; 8]| u64::from_be_bytes(b).saturating_add(1))
        });

        let label_live = db.open_partition(
            "label_live",
            PartitionCreateOptions::default()
                .max_memtable_size(*MEMTABLE_SIZE)
                .block_size(BLOCK_SIZE),
        )?;

        let cursors = db.open_partition("cursors", PartitionCreateOptions::default())?;

        Self::drop_orphan_partitions(&db);

        // Restore the seq read cursor and legacy-drained flag so a restart
        // resumes the forward scan instead of re-walking every tombstone.
        let seq_read_cursor = cursors
            .get(LIVE_SEQ_READ_CURSOR.as_bytes())?
            .and_then(|v| <[u8; 8]>::try_from(v.as_ref()).ok())
            .map(|b| b.to_vec());
        let legacy_drained = cursors.get(LIVE_LEGACY_DRAINED.as_bytes())?.is_some();

        Ok(Self {
            db,
            firehose_events,
            firehose_live,
            firehose_live_seq,
            label_live,
            cursors,
            firehose_live_cursor: std::sync::Mutex::new(None),
            firehose_live_seq_cursor: std::sync::Mutex::new(seq_read_cursor),
            live_seq_next: AtomicU64::new(live_seq_start),
            legacy_live_drained: AtomicBool::new(legacy_drained),
            live_notify: tokio::sync::Notify::new(),
        })
    }

    /// Partitions a previous design left on disk that this binary no longer
    /// opens. Fjall recovers every partition it finds regardless, so an
    /// abandoned queue keeps its segment metadata and block index resident and
    /// takes part in compaction until it is explicitly deleted.
    const ORPHAN_PARTITIONS: [&'static str; 1] = ["repo_backfill"];

    fn drop_orphan_partitions(db: &Keyspace) {
        for name in Self::ORPHAN_PARTITIONS {
            if !db.partition_exists(name) {
                continue;
            }
            let dropped = db
                .open_partition(name, PartitionCreateOptions::default())
                .and_then(|handle| {
                    let bytes = handle.disk_space();
                    db.delete_partition(handle).map(|()| bytes)
                });
            match dropped {
                Ok(bytes) => tracing::info!(
                    "dropped orphaned Fjall partition {name} ({} MB on disk)",
                    bytes / (1024 * 1024)
                ),
                Err(e) => tracing::warn!("failed to drop orphaned Fjall partition {name}: {e}"),
            }
        }
    }

    /// Point-in-time figures for the memory gauges in `procmem`.
    #[must_use]
    pub fn keyspace_stats(&self) -> KeyspaceStats {
        KeyspaceStats {
            write_buffer_bytes: self.db.write_buffer_size(),
            journal_count: self.db.journal_count() as u64,
            disk_bytes: self.db.disk_space(),
        }
    }

    pub fn write_firehose_event(
        &self,
        seq: i64,
        event: &FirehoseEvent,
    ) -> Result<(), WintermuteError> {
        let key = seq.to_be_bytes();
        let mut value = Vec::new();
        ciborium::into_writer(event, &mut value).map_err(|e| {
            WintermuteError::Serialization(format!("failed to serialize event: {e}"))
        })?;
        self.firehose_events.insert(key, value.as_slice())?;
        Ok(())
    }

    pub fn read_firehose_event(&self, seq: i64) -> Result<Option<FirehoseEvent>, WintermuteError> {
        let key = seq.to_be_bytes();
        let Some(value) = self.firehose_events.get(key)? else {
            return Ok(None);
        };
        let event = ciborium::from_reader(value.as_ref())
            .map_err(|e| WintermuteError::Serialization(format!("failed to deserialize: {e}")))?;
        Ok(Some(event))
    }

    // Firehose live queue (from ingester); keys are monotonic so dequeue order
    // is arrival order and the read cursor never revisits tombstones.
    pub fn enqueue_firehose_live(&self, job: &IndexJob) -> Result<(), WintermuteError> {
        let seq = self.live_seq_next.fetch_add(1, Ordering::Relaxed);
        let mut value = Vec::new();
        ciborium::into_writer(job, &mut value)
            .map_err(|e| WintermuteError::Serialization(format!("failed to serialize job: {e}")))?;
        self.firehose_live_seq
            .insert(seq.to_be_bytes(), value.as_slice())?;
        crate::metrics::INGESTER_FIREHOSE_LIVE_LENGTH.inc();
        self.live_notify.notify_one();
        Ok(())
    }

    /// Block until an enqueue signals the live queue, or `timeout` elapses.
    /// A permit stored by a `notify_one` that raced ahead completes immediately.
    pub async fn wait_for_live_enqueue(&self, timeout: std::time::Duration) {
        drop(tokio::time::timeout(timeout, self.live_notify.notified()).await);
    }

    pub fn dequeue_firehose_live(&self) -> Result<Option<(Vec<u8>, IndexJob)>, WintermuteError> {
        let mut batch = self.dequeue_firehose_live_batch(1)?;
        if batch.is_empty() {
            Ok(None)
        } else {
            Ok(Some(batch.remove(0)))
        }
    }

    #[allow(clippy::missing_const_for_fn)]
    pub fn remove_firehose_live(&self, _key: &[u8]) -> Result<(), WintermuteError> {
        // Item already removed in dequeue - this is now a no-op for compatibility
        Ok(())
    }

    fn collect_live_entries(
        iter: impl Iterator<Item = Result<(fjall::Slice, fjall::Slice), fjall::Error>>,
        limit: usize,
        results: &mut Vec<(Vec<u8>, IndexJob)>,
        poisoned: &mut Vec<Vec<u8>>,
    ) -> Result<(), WintermuteError> {
        for entry in iter.take(limit - results.len()) {
            let (key, value) = entry?;
            match ciborium::from_reader(value.as_ref()) {
                Ok(job) => results.push((key.to_vec(), job)),
                Err(e) => {
                    tracing::error!("dropping undeserializable firehose_live entry: {e}");
                    poisoned.push(key.to_vec());
                }
            }
        }
        Ok(())
    }

    /// Dequeue up to `limit` live jobs in arrival order. The legacy uri-keyed
    /// partition drains completely first (existing entries predate the
    /// sequence keys); afterwards the forward-only sequence scan starts after
    /// the last dequeued key, so removal tombstones are never revisited.
    pub fn dequeue_firehose_live_batch(
        &self,
        limit: usize,
    ) -> Result<Vec<(Vec<u8>, IndexJob)>, WintermuteError> {
        let start = std::time::Instant::now();
        let mut legacy = false;
        let results = if self.legacy_live_drained.load(Ordering::Relaxed) {
            self.dequeue_live_seq_batch(limit)?
        } else {
            let legacy_results = self.dequeue_live_legacy_batch(limit)?;
            if legacy_results.is_empty() {
                self.legacy_live_drained.store(true, Ordering::Relaxed);
                if let Err(e) = self
                    .cursors
                    .insert(LIVE_LEGACY_DRAINED.as_bytes(), 1u8.to_be_bytes())
                {
                    tracing::warn!("failed to persist legacy-drained flag: {e}");
                }
                tracing::info!("legacy firehose_live partition drained, switching to seq keys");
                self.dequeue_live_seq_batch(limit)?
            } else {
                legacy = true;
                legacy_results
            }
        };
        let elapsed_ms = start.elapsed().as_millis();
        if elapsed_ms > 1000 {
            tracing::warn!(
                "SLOW live dequeue: {elapsed_ms}ms for {} jobs (legacy={legacy})",
                results.len()
            );
        }
        Ok(results)
    }

    /// Forward-only scan of the sequence-keyed partition.
    fn dequeue_live_seq_batch(
        &self,
        limit: usize,
    ) -> Result<Vec<(Vec<u8>, IndexJob)>, WintermuteError> {
        let cursor = self
            .firehose_live_seq_cursor
            .lock()
            .map_or(None, |c| c.clone());

        let mut results = Vec::with_capacity(limit);
        let mut poisoned: Vec<Vec<u8>> = Vec::new();
        if let Some(after) = cursor {
            let range = (Bound::Excluded(after), Bound::<Vec<u8>>::Unbounded);
            Self::collect_live_entries(
                self.firehose_live_seq.range(range),
                limit,
                &mut results,
                &mut poisoned,
            )?;
        } else {
            Self::collect_live_entries(
                self.firehose_live_seq.iter(),
                limit,
                &mut results,
                &mut poisoned,
            )?;
        }

        let last_key = results
            .last()
            .map(|(k, _)| k.clone())
            .or_else(|| poisoned.last().cloned());
        if let (Some(key), Ok(mut guard)) = (last_key, self.firehose_live_seq_cursor.lock()) {
            self.cursors
                .insert(LIVE_SEQ_READ_CURSOR.as_bytes(), key.as_slice())?;
            *guard = Some(key);
        } else if results.is_empty() && poisoned.is_empty() {
            // A wiped-and-recreated partition restarts seq keys from zero; a
            // persisted cursor from before the wipe would then skip everything.
            if let Some((first, _)) = self.firehose_live_seq.first_key_value()? {
                let stale = self
                    .firehose_live_seq_cursor
                    .lock()
                    .ok()
                    .and_then(|g| g.clone())
                    .is_some_and(|c| first.as_ref() < c.as_slice());
                if stale {
                    tracing::warn!("live seq read cursor is ahead of the partition; resetting");
                    if let Ok(mut guard) = self.firehose_live_seq_cursor.lock() {
                        *guard = None;
                    }
                    self.cursors.remove(LIVE_SEQ_READ_CURSOR.as_bytes())?;
                }
            }
        }

        for (key, _) in &results {
            self.firehose_live_seq.remove(key.as_slice())?;
        }
        for key in &poisoned {
            self.firehose_live_seq.remove(key.as_slice())?;
        }

        #[allow(clippy::cast_possible_wrap)]
        crate::metrics::INGESTER_FIREHOSE_LIVE_LENGTH.sub((results.len() + poisoned.len()) as i64);
        Ok(results)
    }

    /// Sweep-cursor scan of the legacy uri-keyed partition: resumes after the
    /// last dequeued key and wraps to the partition start when exhausted.
    /// Undeserializable entries are removed and skipped so a poison entry
    /// cannot wedge the sweep.
    fn dequeue_live_legacy_batch(
        &self,
        limit: usize,
    ) -> Result<Vec<(Vec<u8>, IndexJob)>, WintermuteError> {
        let cursor = self.firehose_live_cursor.lock().map_or(None, |c| c.clone());

        let mut results = Vec::with_capacity(limit);
        let mut poisoned: Vec<Vec<u8>> = Vec::new();

        if let Some(after) = cursor {
            let range = (
                std::ops::Bound::Excluded(after),
                std::ops::Bound::<Vec<u8>>::Unbounded,
            );
            Self::collect_live_entries(
                self.firehose_live.range(range),
                limit,
                &mut results,
                &mut poisoned,
            )?;
        }
        if results.is_empty() && poisoned.is_empty() {
            // Wrap to the partition start to sweep keys behind the cursor
            Self::collect_live_entries(
                self.firehose_live.iter(),
                limit,
                &mut results,
                &mut poisoned,
            )?;
        }

        let last_key = results
            .last()
            .map(|(k, _)| k.clone())
            .or_else(|| poisoned.last().cloned());
        if let (Some(key), Ok(mut guard)) = (last_key, self.firehose_live_cursor.lock()) {
            *guard = Some(key);
        }

        for (key, _) in &results {
            self.firehose_live.remove(key.as_slice())?;
        }
        for key in &poisoned {
            self.firehose_live.remove(key.as_slice())?;
        }

        #[allow(clippy::cast_possible_wrap)]
        crate::metrics::INGESTER_FIREHOSE_LIVE_LENGTH.sub((results.len() + poisoned.len()) as i64);
        Ok(results)
    }

    // Firehose backfill queue (from backfiller) - uses LMDB for consistent sub-ms iteration
    // Uses key-prefix partitioning for fast parallel dequeue:
    // - Priority items: prefix "0:" (processed first by all workers)
    // - Normal items: prefix "{XX}:" where XX is random hex in range 10-ff
    // Workers claim partitions of the key space for contention-free dequeue

    // Label live queue (future implementation)
    pub fn enqueue_label_live(
        &self,
        event: &crate::types::LabelEvent,
    ) -> Result<(), WintermuteError> {
        let key = format!("{}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
        let mut value = Vec::new();
        ciborium::into_writer(event, &mut value).map_err(|e| {
            WintermuteError::Serialization(format!("failed to serialize event: {e}"))
        })?;
        self.label_live.insert(key.as_bytes(), value.as_slice())?;
        crate::metrics::INGESTER_LABEL_LIVE_LENGTH.inc();
        Ok(())
    }

    pub fn dequeue_label_live(
        &self,
    ) -> Result<Option<(Vec<u8>, crate::types::LabelEvent)>, WintermuteError> {
        let mut iter = self.label_live.iter();
        let Some(entry) = iter.next() else {
            return Ok(None);
        };
        let (key, value) = entry?;
        let key_vec = key.to_vec();
        let event = ciborium::from_reader(value.as_ref())
            .map_err(|e| WintermuteError::Serialization(format!("failed to deserialize: {e}")))?;
        // Remove immediately to prevent re-dequeue race condition
        self.label_live.remove(&key_vec)?;
        crate::metrics::INGESTER_LABEL_LIVE_LENGTH.dec();
        Ok(Some((key_vec, event)))
    }

    #[allow(clippy::missing_const_for_fn)]
    pub fn remove_label_live(&self, _key: &[u8]) -> Result<(), WintermuteError> {
        // Item already removed in dequeue - this is now a no-op for compatibility
        Ok(())
    }

    pub fn get_cursor(&self, name: &str) -> Result<Option<i64>, WintermuteError> {
        let Some(value) = self.cursors.get(name.as_bytes())? else {
            return Ok(None);
        };
        let bytes: [u8; 8] = value
            .as_ref()
            .try_into()
            .map_err(|_| WintermuteError::Other("invalid cursor format".into()))?;
        Ok(Some(i64::from_be_bytes(bytes)))
    }

    pub fn set_cursor(&self, name: &str, value: i64) -> Result<(), WintermuteError> {
        self.cursors.insert(name.as_bytes(), value.to_be_bytes())?;
        Ok(())
    }

    pub fn delete_cursor(&self, name: &str) -> Result<(), WintermuteError> {
        self.cursors.remove(name.as_bytes())?;
        Ok(())
    }

    pub fn firehose_live_len(&self) -> Result<usize, WintermuteError> {
        Ok(self.firehose_live.len()? + self.firehose_live_seq.len()?)
    }

    pub fn label_live_len(&self) -> Result<usize, WintermuteError> {
        Ok(self.label_live.len()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CommitData, FirehoseEvent, IndexJob, Label, LabelEvent, WriteAction};
    use tempfile::TempDir;

    fn setup_test_storage() -> (Storage, TempDir) {
        let temp_dir = TempDir::with_prefix("wintermute_test_").unwrap();
        let db_path = temp_dir.path().join("test_db");
        let storage = Storage::new(Some(db_path)).unwrap();
        (storage, temp_dir)
    }

    #[test]
    fn test_firehose_event_roundtrip() {
        let (storage, _dir) = setup_test_storage();

        let event = FirehoseEvent {
            seq: 12345,
            did: "did:plc:test123".to_owned(),
            time: "2025-01-01T00:00:00Z".to_owned(),
            kind: "commit".to_owned(),
            commit: Some(CommitData {
                rev: "rev123".to_owned(),
                ops: vec![],
                blocks: vec![],
                since: None,
                prev_data: None,
                data: None,
                too_big: false,
            }),
            identity: None,
            account: None,
        };

        storage.write_firehose_event(12345, &event).unwrap();
        let retrieved = storage.read_firehose_event(12345).unwrap();

        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.seq, event.seq);
        assert_eq!(retrieved.did, event.did);
    }

    #[test]
    fn test_firehose_live_queue() {
        let (storage, _dir) = setup_test_storage();

        let job = IndexJob {
            uri: "at://did:plc:test/app.bsky.feed.post/123".to_owned(),
            cid: "bafytest123".to_owned(),
            action: WriteAction::Create,
            record: Some(serde_json::json!({"test": "data"})),
            indexed_at: "2025-01-01T00:00:00Z".to_owned(),
            rev: "rev123".to_owned(),
        };

        storage.enqueue_firehose_live(&job).unwrap();
        let (key, retrieved) = storage.dequeue_firehose_live().unwrap().unwrap();

        assert_eq!(retrieved.uri, job.uri);
        assert_eq!(retrieved.cid, job.cid);

        storage.remove_firehose_live(&key).unwrap();
        assert!(storage.dequeue_firehose_live().unwrap().is_none());
    }

    #[test]
    fn test_label_live_queue() {
        let (storage, _dir) = setup_test_storage();

        let event = LabelEvent {
            seq: 789,
            labels: vec![Label {
                src: "did:plc:labeler".to_owned(),
                uri: "at://did:plc:test/app.bsky.feed.post/123".to_owned(),
                cid: None,
                val: "spam".to_owned(),
                neg: false,
                cts: "2025-01-01T00:00:00Z".to_owned(),
                exp: None,
            }],
        };

        storage.enqueue_label_live(&event).unwrap();
        let (key, retrieved) = storage.dequeue_label_live().unwrap().unwrap();

        assert_eq!(retrieved.seq, event.seq);
        assert_eq!(retrieved.labels.len(), 1);

        storage.remove_label_live(&key).unwrap();
        assert!(storage.dequeue_label_live().unwrap().is_none());
    }

    #[test]
    fn test_cursor() {
        let (storage, _dir) = setup_test_storage();

        assert!(storage.get_cursor("test").unwrap().is_none());

        storage.set_cursor("test", 42).unwrap();
        assert_eq!(storage.get_cursor("test").unwrap(), Some(42));

        storage.set_cursor("test", 100).unwrap();
        assert_eq!(storage.get_cursor("test").unwrap(), Some(100));
    }

    #[test]
    fn test_delete_cursor() {
        let (storage, _dir) = setup_test_storage();

        storage.set_cursor("test_delete", 42).unwrap();
        assert_eq!(storage.get_cursor("test_delete").unwrap(), Some(42));

        storage.delete_cursor("test_delete").unwrap();
        assert!(storage.get_cursor("test_delete").unwrap().is_none());
    }

    #[test]
    fn test_is_storage_corrupted() {
        // Test that is_storage_corrupted returns true for Storage errors with corruption indicators
        let poisoned_err: WintermuteError = fjall::Error::Poisoned.into();
        assert!(
            poisoned_err.is_storage_corrupted(),
            "Poisoned should be detected"
        );

        // Test JournalRecovery error (simulated via io error that wraps into JournalRecovery)
        let io_err = std::io::Error::other("journal issue");
        let storage_err: WintermuteError = fjall::Error::Io(io_err).into();
        // IO errors are not corruption
        assert!(
            !storage_err.is_storage_corrupted(),
            "IO errors should not be detected as corruption"
        );

        // Test non-corruption errors
        let other_err = WintermuteError::Other("some error".to_owned());
        assert!(
            !other_err.is_storage_corrupted(),
            "Other errors should not be detected as corruption"
        );

        let serial_err = WintermuteError::Serialization("bad data".to_owned());
        assert!(
            !serial_err.is_storage_corrupted(),
            "Serialization errors should not be detected as corruption"
        );
    }

    #[test]
    fn test_storage_recovery_from_corruption() {
        // Test that Storage::new successfully creates fresh storage after first open fails
        // We can't easily simulate fjall corruption, but we can test the happy path
        let temp_dir = TempDir::with_prefix("storage_recovery_test_").unwrap();
        let db_path = temp_dir.path().join("test_db");

        // First creation should succeed
        let storage = Storage::new(Some(db_path.clone())).unwrap();
        storage.set_cursor("test", 42).unwrap();
        drop(storage);

        // Second creation should also succeed (reopen)
        let storage2 = Storage::new(Some(db_path)).unwrap();
        assert_eq!(
            storage2.get_cursor("test").unwrap(),
            Some(42),
            "should preserve data on reopen"
        );
    }

    fn live_job(uri: &str) -> IndexJob {
        IndexJob {
            uri: uri.to_owned(),
            cid: "cid".to_owned(),
            action: WriteAction::Create,
            record: None,
            indexed_at: "2026-08-01T00:00:00.000Z".to_owned(),
            rev: "3a".to_owned(),
        }
    }

    #[test]
    fn live_seq_read_cursor_persists_across_reopen() {
        let dir = TempDir::with_prefix("wintermute_test_").unwrap();
        let db_path = dir.path().join("test_db");
        let storage = Storage::new(Some(db_path.clone())).unwrap();
        for i in 0..6 {
            storage
                .enqueue_firehose_live(&live_job(&format!(
                    "at://did:plc:a/app.bsky.feed.post/p{i}"
                )))
                .unwrap();
        }
        assert_eq!(storage.dequeue_firehose_live_batch(4).unwrap().len(), 4);
        drop(storage);

        let storage = Storage::new(Some(db_path)).unwrap();
        let resumed = storage.dequeue_firehose_live_batch(10).unwrap();
        assert_eq!(
            resumed.len(),
            2,
            "reopen must resume after the persisted cursor"
        );
        assert_eq!(resumed[0].1.uri, "at://did:plc:a/app.bsky.feed.post/p4");
        assert_eq!(resumed[1].1.uri, "at://did:plc:a/app.bsky.feed.post/p5");
        drop(dir);
    }

    #[test]
    fn stale_read_cursor_resets_when_partition_restarts() {
        let dir = TempDir::with_prefix("wintermute_test_").unwrap();
        let db_path = dir.path().join("test_db");
        let storage = Storage::new(Some(db_path.clone())).unwrap();
        for i in 0..3 {
            storage
                .enqueue_firehose_live(&live_job(&format!(
                    "at://did:plc:a/app.bsky.feed.post/q{i}"
                )))
                .unwrap();
        }
        assert_eq!(storage.dequeue_firehose_live_batch(10).unwrap().len(), 3);
        // Simulate the runbook partition wipe: force a cursor far ahead of any
        // key the recreated partition will produce.
        storage
            .cursors
            .insert(LIVE_SEQ_READ_CURSOR.as_bytes(), 1_000_000u64.to_be_bytes())
            .unwrap();
        drop(storage);

        let storage = Storage::new(Some(db_path)).unwrap();
        storage
            .enqueue_firehose_live(&live_job("at://did:plc:a/app.bsky.feed.post/fresh"))
            .unwrap();
        // Fresh key sorts below the stale cursor: first dequeue detects and
        // resets, second dequeue returns the job.
        let first = storage.dequeue_firehose_live_batch(10).unwrap();
        let second = storage.dequeue_firehose_live_batch(10).unwrap();
        let total = first.len() + second.len();
        assert_eq!(total, 1, "job behind a stale cursor must not be skipped");
        drop(dir);
    }

    #[test]
    fn live_queue_dequeues_in_arrival_order() {
        let (storage, _dir) = setup_test_storage();
        for i in 0..5 {
            storage
                .enqueue_firehose_live(&live_job(&format!(
                    "at://did:plc:a/app.bsky.feed.post/r{i}"
                )))
                .unwrap();
        }
        let batch = storage.dequeue_firehose_live_batch(10).unwrap();
        assert_eq!(batch.len(), 5);
        for (i, (key, job)) in batch.iter().enumerate() {
            assert_eq!(job.uri, format!("at://did:plc:a/app.bsky.feed.post/r{i}"));
            assert_eq!(key.len(), 8);
        }
        storage
            .enqueue_firehose_live(&live_job("at://did:plc:a/app.bsky.feed.post/r5"))
            .unwrap();
        let next = storage.dequeue_firehose_live_batch(10).unwrap();
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].1.uri, "at://did:plc:a/app.bsky.feed.post/r5");
        assert!(storage.dequeue_firehose_live_batch(10).unwrap().is_empty());
    }

    #[test]
    fn live_queue_drains_legacy_partition_first() {
        let (storage, _dir) = setup_test_storage();
        for i in 0..3 {
            let job = live_job(&format!("at://did:plc:legacy/app.bsky.feed.post/l{i}"));
            let mut value = Vec::new();
            ciborium::into_writer(&job, &mut value).unwrap();
            storage
                .firehose_live
                .insert(format!("{}:{i}", job.uri).as_bytes(), value.as_slice())
                .unwrap();
        }
        storage
            .enqueue_firehose_live(&live_job("at://did:plc:new/app.bsky.feed.post/n0"))
            .unwrap();
        assert_eq!(storage.firehose_live_len().unwrap(), 4);

        let first = storage.dequeue_firehose_live_batch(10).unwrap();
        assert_eq!(first.len(), 3);
        assert!(first.iter().all(|(_, j)| j.uri.contains("did:plc:legacy")));

        let second = storage.dequeue_firehose_live_batch(10).unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].1.uri, "at://did:plc:new/app.bsky.feed.post/n0");
        assert!(storage.legacy_live_drained.load(Ordering::Relaxed));
        assert_eq!(storage.firehose_live_len().unwrap(), 0);
    }

    #[test]
    fn orphaned_repo_backfill_partition_is_dropped_on_open() {
        let dir = TempDir::with_prefix("wintermute_test_").unwrap();
        let db_path = dir.path().join("test_db");
        {
            // Simulate a store written by the previous design: the daemon
            // keyspace plus the abandoned repo_backfill queue.
            let storage = Storage::new(Some(db_path.clone())).unwrap();
            let orphan = storage
                .db
                .open_partition("repo_backfill", PartitionCreateOptions::default())
                .unwrap();
            orphan.insert(b"did:plc:x", b"queued").unwrap();
            storage.db.persist(fjall::PersistMode::SyncAll).unwrap();
            assert!(storage.db.partition_exists("repo_backfill"));
        }

        let storage = Storage::new(Some(db_path.clone())).unwrap();
        assert!(
            !storage.db.partition_exists("repo_backfill"),
            "reopen must delete the orphaned partition"
        );
        for live in [
            "firehose_live",
            "firehose_live_seq",
            "label_live",
            "cursors",
        ] {
            assert!(storage.db.partition_exists(live), "{live} must survive");
        }
        drop(storage);

        // Idempotent: a second open with nothing to drop is fine.
        let storage = Storage::new(Some(db_path)).unwrap();
        assert!(!storage.db.partition_exists("repo_backfill"));
        drop(dir);
    }

    #[test]
    fn keyspace_stats_report_live_figures() {
        let (storage, _dir) = setup_test_storage();
        let before = storage.keyspace_stats();
        for i in 0..50 {
            storage
                .enqueue_firehose_live(&live_job(&format!(
                    "at://did:plc:a/app.bsky.feed.post/s{i}"
                )))
                .unwrap();
        }
        let after = storage.keyspace_stats();
        assert!(
            after.write_buffer_bytes > before.write_buffer_bytes,
            "enqueued jobs must show up in the memtable figure"
        );
        assert!(after.journal_count >= 1);
    }

    #[test]
    fn live_queue_keys_stay_monotonic_across_reopen() {
        let temp_dir = TempDir::with_prefix("wintermute_test_").unwrap();
        let db_path = temp_dir.path().join("test_db");
        {
            let storage = Storage::new(Some(db_path.clone())).unwrap();
            storage
                .enqueue_firehose_live(&live_job("at://did:plc:a/app.bsky.feed.post/first"))
                .unwrap();
        }
        let storage = Storage::new(Some(db_path)).unwrap();
        storage
            .enqueue_firehose_live(&live_job("at://did:plc:a/app.bsky.feed.post/second"))
            .unwrap();
        let batch = storage.dequeue_firehose_live_batch(10).unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].1.uri, "at://did:plc:a/app.bsky.feed.post/first");
        assert_eq!(batch[1].1.uri, "at://did:plc:a/app.bsky.feed.post/second");
        assert!(batch[0].0 < batch[1].0);
    }
}
