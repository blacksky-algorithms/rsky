use crate::config::{BLOCK_SIZE, CACHE_SIZE, FSYNC_MS, MEMTABLE_SIZE, WRITE_BUFFER_SIZE};
use crate::types::{BackfillJob, FirehoseEvent, IndexJob, WintermuteError};
use fjall::{Config, Keyspace, PartitionCreateOptions, PartitionHandle};
use std::path::PathBuf;
use std::sync::Arc;

pub struct Storage {
    _db: Arc<Keyspace>,
    firehose_events: PartitionHandle,
    repo_backfill: PartitionHandle,
    firehose_live: PartitionHandle,
    firehose_backfill: PartitionHandle,
    label_live: PartitionHandle,
    cursors: PartitionHandle,
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
        let db = Config::new(path)
            .cache_size(CACHE_SIZE)
            .max_write_buffer_size(WRITE_BUFFER_SIZE)
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
                .max_memtable_size(MEMTABLE_SIZE)
                .block_size(BLOCK_SIZE),
        )?;

        let repo_backfill = db.open_partition(
            "repo_backfill",
            PartitionCreateOptions::default()
                .max_memtable_size(MEMTABLE_SIZE)
                .block_size(BLOCK_SIZE),
        )?;

        let firehose_live = db.open_partition(
            "firehose_live",
            PartitionCreateOptions::default()
                .max_memtable_size(MEMTABLE_SIZE)
                .block_size(BLOCK_SIZE),
        )?;

        let firehose_backfill = db.open_partition(
            "firehose_backfill",
            PartitionCreateOptions::default()
                .max_memtable_size(MEMTABLE_SIZE)
                .block_size(BLOCK_SIZE),
        )?;

        let label_live = db.open_partition(
            "label_live",
            PartitionCreateOptions::default()
                .max_memtable_size(MEMTABLE_SIZE)
                .block_size(BLOCK_SIZE),
        )?;

        let cursors = db.open_partition("cursors", PartitionCreateOptions::default())?;

        Ok(Self {
            _db: db,
            firehose_events,
            repo_backfill,
            firehose_live,
            firehose_backfill,
            label_live,
            cursors,
        })
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

    /// Enqueue a backfill job with normal priority (prefix "1:")
    /// Normal priority items are processed after all priority items
    pub fn enqueue_backfill(&self, job: &BackfillJob) -> Result<(), WintermuteError> {
        // Key format: "1:{timestamp}:{did}" - "1:" prefix for normal priority
        // Timestamp first ensures FIFO ordering within priority level
        let key = format!(
            "1:{}:{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
            job.did
        );
        let mut value = Vec::new();
        ciborium::into_writer(job, &mut value)
            .map_err(|e| WintermuteError::Serialization(format!("failed to serialize job: {e}")))?;
        self.repo_backfill
            .insert(key.as_bytes(), value.as_slice())?;
        crate::metrics::INGESTER_REPO_BACKFILL_LENGTH.inc();
        Ok(())
    }

    /// Enqueue a backfill job with HIGH priority (prefix "0:")
    /// Priority items are processed BEFORE all normal items
    /// Use this for manual/on-demand backfill requests
    pub fn enqueue_backfill_priority(&self, job: &BackfillJob) -> Result<(), WintermuteError> {
        // Key format: "0:{timestamp}:{did}" - "0:" prefix for high priority
        // Timestamp ensures FIFO ordering within priority level
        let key = format!(
            "0:{}:{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
            job.did
        );
        let mut value = Vec::new();
        ciborium::into_writer(job, &mut value)
            .map_err(|e| WintermuteError::Serialization(format!("failed to serialize job: {e}")))?;
        self.repo_backfill
            .insert(key.as_bytes(), value.as_slice())?;
        crate::metrics::INGESTER_REPO_BACKFILL_LENGTH.inc();
        Ok(())
    }

    pub fn dequeue_backfill(&self) -> Result<Option<(Vec<u8>, BackfillJob)>, WintermuteError> {
        let mut iter = self.repo_backfill.iter();
        let Some(entry) = iter.next() else {
            return Ok(None);
        };
        let (key, value) = entry?;
        let key_vec = key.to_vec();
        let job = ciborium::from_reader(value.as_ref())
            .map_err(|e| WintermuteError::Serialization(format!("failed to deserialize: {e}")))?;
        // Remove immediately to prevent re-dequeue race condition
        self.repo_backfill.remove(&key_vec)?;
        crate::metrics::INGESTER_REPO_BACKFILL_LENGTH.dec();
        Ok(Some((key_vec, job)))
    }

    #[allow(clippy::missing_const_for_fn)]
    pub fn remove_backfill(&self, _key: &[u8]) -> Result<(), WintermuteError> {
        // Item already removed in dequeue - this is now a no-op for compatibility
        Ok(())
    }

    // Firehose live queue (from ingester)
    pub fn enqueue_firehose_live(&self, job: &IndexJob) -> Result<(), WintermuteError> {
        let key = format!(
            "{}:{}",
            job.uri,
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        );
        let mut value = Vec::new();
        ciborium::into_writer(job, &mut value)
            .map_err(|e| WintermuteError::Serialization(format!("failed to serialize job: {e}")))?;
        self.firehose_live
            .insert(key.as_bytes(), value.as_slice())?;
        crate::metrics::INGESTER_FIREHOSE_LIVE_LENGTH.inc();
        Ok(())
    }

    pub fn dequeue_firehose_live(&self) -> Result<Option<(Vec<u8>, IndexJob)>, WintermuteError> {
        let mut iter = self.firehose_live.iter();
        let Some(entry) = iter.next() else {
            return Ok(None);
        };
        let (key, value) = entry?;
        let key_vec = key.to_vec();
        let job = ciborium::from_reader(value.as_ref())
            .map_err(|e| WintermuteError::Serialization(format!("failed to deserialize: {e}")))?;
        // Remove immediately to prevent re-dequeue race condition
        self.firehose_live.remove(&key_vec)?;
        crate::metrics::INGESTER_FIREHOSE_LIVE_LENGTH.dec();
        Ok(Some((key_vec, job)))
    }

    #[allow(clippy::missing_const_for_fn)]
    pub fn remove_firehose_live(&self, _key: &[u8]) -> Result<(), WintermuteError> {
        // Item already removed in dequeue - this is now a no-op for compatibility
        Ok(())
    }

    // Firehose backfill queue (from backfiller)
    /// Enqueue a firehose backfill job with normal priority (prefix "1:")
    pub fn enqueue_firehose_backfill(&self, job: &IndexJob) -> Result<(), WintermuteError> {
        // Key format: "1:{timestamp}:{uri}" - "1:" prefix for normal priority
        let key = format!(
            "1:{}:{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
            job.uri
        );
        let mut value = Vec::new();
        ciborium::into_writer(job, &mut value)
            .map_err(|e| WintermuteError::Serialization(format!("failed to serialize job: {e}")))?;
        self.firehose_backfill
            .insert(key.as_bytes(), value.as_slice())?;
        crate::metrics::INGESTER_FIREHOSE_BACKFILL_LENGTH.inc();
        Ok(())
    }

    /// Enqueue a firehose backfill job with HIGH priority (prefix "0:")
    /// Priority items are indexed BEFORE all normal backfill items
    pub fn enqueue_firehose_backfill_priority(
        &self,
        job: &IndexJob,
    ) -> Result<(), WintermuteError> {
        // Key format: "0:{timestamp}:{uri}" - "0:" prefix for high priority
        let key = format!(
            "0:{}:{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
            job.uri
        );
        let mut value = Vec::new();
        ciborium::into_writer(job, &mut value)
            .map_err(|e| WintermuteError::Serialization(format!("failed to serialize job: {e}")))?;
        self.firehose_backfill
            .insert(key.as_bytes(), value.as_slice())?;
        crate::metrics::INGESTER_FIREHOSE_BACKFILL_LENGTH.inc();
        Ok(())
    }

    pub fn dequeue_firehose_backfill(
        &self,
    ) -> Result<Option<(Vec<u8>, IndexJob)>, WintermuteError> {
        let mut iter = self.firehose_backfill.iter();
        let Some(entry) = iter.next() else {
            return Ok(None);
        };
        let (key, value) = entry?;
        let key_vec = key.to_vec();
        let job = ciborium::from_reader(value.as_ref())
            .map_err(|e| WintermuteError::Serialization(format!("failed to deserialize: {e}")))?;
        // Remove immediately to prevent re-dequeue race condition
        self.firehose_backfill.remove(&key_vec)?;
        crate::metrics::INGESTER_FIREHOSE_BACKFILL_LENGTH.dec();
        Ok(Some((key_vec, job)))
    }

    /// Batch dequeue up to `limit` jobs from `firehose_backfill` in a single iteration
    /// This reduces Fjall lock contention compared to calling `dequeue_firehose_backfill()` N times
    pub fn dequeue_firehose_backfill_batch(
        &self,
        limit: usize,
    ) -> Result<Vec<(Vec<u8>, IndexJob)>, WintermuteError> {
        let mut results = Vec::with_capacity(limit);
        let mut iter = self.firehose_backfill.iter();

        for _ in 0..limit {
            let Some(entry) = iter.next() else {
                break;
            };
            let (key, value) = entry?;
            let key_vec = key.to_vec();
            let job: IndexJob = ciborium::from_reader(value.as_ref()).map_err(|e| {
                WintermuteError::Serialization(format!("failed to deserialize: {e}"))
            })?;
            results.push((key_vec, job));
        }

        // Remove all dequeued items
        for (key, _) in &results {
            self.firehose_backfill.remove(key)?;
            crate::metrics::INGESTER_FIREHOSE_BACKFILL_LENGTH.dec();
        }

        Ok(results)
    }

    #[allow(clippy::missing_const_for_fn)]
    pub fn remove_firehose_backfill(&self, _key: &[u8]) -> Result<(), WintermuteError> {
        // Item already removed in dequeue - this is now a no-op for compatibility
        Ok(())
    }

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

    pub fn repo_backfill_len(&self) -> Result<usize, WintermuteError> {
        Ok(self.repo_backfill.len()?)
    }

    pub fn firehose_live_len(&self) -> Result<usize, WintermuteError> {
        Ok(self.firehose_live.len()?)
    }

    pub fn firehose_backfill_len(&self) -> Result<usize, WintermuteError> {
        Ok(self.firehose_backfill.len()?)
    }

    pub fn label_live_len(&self) -> Result<usize, WintermuteError> {
        Ok(self.label_live.len()?)
    }

    /// Peek at the first N items in `repo_backfill` without removing them
    pub fn peek_backfill(
        &self,
        limit: usize,
    ) -> Result<Vec<(Vec<u8>, BackfillJob)>, WintermuteError> {
        let mut results = Vec::with_capacity(limit);
        let mut iter = self.repo_backfill.iter();
        for _ in 0..limit {
            let Some(entry) = iter.next() else {
                break;
            };
            let (key, value) = entry?;
            let key_vec = key.to_vec();
            let job: BackfillJob = ciborium::from_reader(value.as_ref())
                .map_err(|e| WintermuteError::Serialization(format!("deserialize failed: {e}")))?;
            results.push((key_vec, job));
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        BackfillJob, CommitData, FirehoseEvent, IndexJob, Label, LabelEvent, WriteAction,
    };
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
            }),
        };

        storage.write_firehose_event(12345, &event).unwrap();
        let retrieved = storage.read_firehose_event(12345).unwrap();

        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.seq, event.seq);
        assert_eq!(retrieved.did, event.did);
    }

    #[test]
    fn test_backfill_queue() {
        let (storage, _dir) = setup_test_storage();

        let job = BackfillJob {
            did: "did:plc:test456".to_owned(),
            retry_count: 0,
            priority: false,
        };

        storage.enqueue_backfill(&job).unwrap();
        let (key, retrieved) = storage.dequeue_backfill().unwrap().unwrap();

        assert_eq!(retrieved.did, job.did);
        assert_eq!(retrieved.retry_count, job.retry_count);

        storage.remove_backfill(&key).unwrap();
        assert!(storage.dequeue_backfill().unwrap().is_none());
    }

    #[test]
    fn test_backfill_priority_queue() {
        let (storage, _dir) = setup_test_storage();

        // First, enqueue normal priority items
        let normal1 = BackfillJob {
            did: "did:plc:normal1".to_owned(),
            retry_count: 0,
            priority: false,
        };
        let normal2 = BackfillJob {
            did: "did:plc:normal2".to_owned(),
            retry_count: 0,
            priority: false,
        };
        storage.enqueue_backfill(&normal1).unwrap();
        storage.enqueue_backfill(&normal2).unwrap();

        // Then, enqueue priority items (should come out FIRST despite being added later)
        let priority1 = BackfillJob {
            did: "did:plc:priority1".to_owned(),
            retry_count: 0,
            priority: true,
        };
        let priority2 = BackfillJob {
            did: "did:plc:priority2".to_owned(),
            retry_count: 0,
            priority: true,
        };
        storage.enqueue_backfill_priority(&priority1).unwrap();
        storage.enqueue_backfill_priority(&priority2).unwrap();

        // Dequeue should return priority items first (0: prefix sorts before 1:)
        let (_, first) = storage.dequeue_backfill().unwrap().unwrap();
        assert_eq!(
            first.did, "did:plc:priority1",
            "priority1 should come first"
        );

        let (_, second) = storage.dequeue_backfill().unwrap().unwrap();
        assert_eq!(
            second.did, "did:plc:priority2",
            "priority2 should come second"
        );

        // Then normal items
        let (_, third) = storage.dequeue_backfill().unwrap().unwrap();
        assert_eq!(third.did, "did:plc:normal1", "normal1 should come third");

        let (_, fourth) = storage.dequeue_backfill().unwrap().unwrap();
        assert_eq!(fourth.did, "did:plc:normal2", "normal2 should come fourth");

        // Queue should be empty
        assert!(storage.dequeue_backfill().unwrap().is_none());
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
    fn test_firehose_backfill_queue() {
        let (storage, _dir) = setup_test_storage();

        let job = IndexJob {
            uri: "at://did:plc:test/app.bsky.feed.post/456".to_owned(),
            cid: "bafytest456".to_owned(),
            action: WriteAction::Create,
            record: Some(serde_json::json!({"test": "backfill"})),
            indexed_at: "2025-01-01T00:00:00Z".to_owned(),
            rev: "rev456".to_owned(),
        };

        storage.enqueue_firehose_backfill(&job).unwrap();
        let (key, retrieved) = storage.dequeue_firehose_backfill().unwrap().unwrap();

        assert_eq!(retrieved.uri, job.uri);
        assert_eq!(retrieved.cid, job.cid);

        storage.remove_firehose_backfill(&key).unwrap();
        assert!(storage.dequeue_firehose_backfill().unwrap().is_none());
    }

    #[test]
    fn test_firehose_backfill_priority_queue() {
        let (storage, _dir) = setup_test_storage();

        // First, enqueue normal priority items
        let normal1 = IndexJob {
            uri: "at://did:plc:normal/app.bsky.feed.post/1".to_owned(),
            cid: "bafynormal1".to_owned(),
            action: WriteAction::Create,
            record: Some(serde_json::json!({"test": "normal1"})),
            indexed_at: "2025-01-01T00:00:00Z".to_owned(),
            rev: "rev1".to_owned(),
        };
        let normal2 = IndexJob {
            uri: "at://did:plc:normal/app.bsky.feed.post/2".to_owned(),
            cid: "bafynormal2".to_owned(),
            action: WriteAction::Create,
            record: Some(serde_json::json!({"test": "normal2"})),
            indexed_at: "2025-01-01T00:00:00Z".to_owned(),
            rev: "rev2".to_owned(),
        };
        storage.enqueue_firehose_backfill(&normal1).unwrap();
        storage.enqueue_firehose_backfill(&normal2).unwrap();

        // Then, enqueue priority items (should come out FIRST despite being added later)
        let priority1 = IndexJob {
            uri: "at://did:plc:priority/app.bsky.feed.post/1".to_owned(),
            cid: "bafypriority1".to_owned(),
            action: WriteAction::Create,
            record: Some(serde_json::json!({"test": "priority1"})),
            indexed_at: "2025-01-01T00:00:00Z".to_owned(),
            rev: "rev3".to_owned(),
        };
        let priority2 = IndexJob {
            uri: "at://did:plc:priority/app.bsky.feed.post/2".to_owned(),
            cid: "bafypriority2".to_owned(),
            action: WriteAction::Create,
            record: Some(serde_json::json!({"test": "priority2"})),
            indexed_at: "2025-01-01T00:00:00Z".to_owned(),
            rev: "rev4".to_owned(),
        };
        storage
            .enqueue_firehose_backfill_priority(&priority1)
            .unwrap();
        storage
            .enqueue_firehose_backfill_priority(&priority2)
            .unwrap();

        // Dequeue should return priority items first (0: prefix sorts before 1:)
        let (_, first) = storage.dequeue_firehose_backfill().unwrap().unwrap();
        assert!(
            first.uri.contains("priority"),
            "priority item should come first, got: {}",
            first.uri
        );

        let (_, second) = storage.dequeue_firehose_backfill().unwrap().unwrap();
        assert!(
            second.uri.contains("priority"),
            "priority item should come second, got: {}",
            second.uri
        );

        // Then normal items
        let (_, third) = storage.dequeue_firehose_backfill().unwrap().unwrap();
        assert!(
            third.uri.contains("normal"),
            "normal item should come third, got: {}",
            third.uri
        );

        let (_, fourth) = storage.dequeue_firehose_backfill().unwrap().unwrap();
        assert!(
            fourth.uri.contains("normal"),
            "normal item should come fourth, got: {}",
            fourth.uri
        );

        // Queue should be empty
        assert!(storage.dequeue_firehose_backfill().unwrap().is_none());
    }

    #[test]
    fn test_firehose_backfill_batch_dequeue() {
        let (storage, _dir) = setup_test_storage();

        // Enqueue 10 jobs
        for i in 0..10 {
            let job = IndexJob {
                uri: format!("at://did:plc:test/app.bsky.feed.post/{i}"),
                cid: format!("bafytest{i}"),
                action: WriteAction::Create,
                record: Some(serde_json::json!({"index": i})),
                indexed_at: "2025-01-01T00:00:00Z".to_owned(),
                rev: format!("rev{i}"),
            };
            storage.enqueue_firehose_backfill(&job).unwrap();
        }

        // Batch dequeue 5
        let batch1 = storage.dequeue_firehose_backfill_batch(5).unwrap();
        assert_eq!(batch1.len(), 5, "should dequeue exactly 5 items");

        // Batch dequeue remaining 5
        let batch2 = storage.dequeue_firehose_backfill_batch(5).unwrap();
        assert_eq!(batch2.len(), 5, "should dequeue remaining 5 items");

        // Queue should be empty
        let batch3 = storage.dequeue_firehose_backfill_batch(5).unwrap();
        assert_eq!(batch3.len(), 0, "should return empty when queue is empty");

        // Regular dequeue should also return None
        assert!(storage.dequeue_firehose_backfill().unwrap().is_none());
    }

    #[test]
    fn test_label_live_queue() {
        let (storage, _dir) = setup_test_storage();

        let event = LabelEvent {
            seq: 789,
            labels: vec![Label {
                src: "did:plc:labeler".to_owned(),
                uri: "at://did:plc:test/app.bsky.feed.post/123".to_owned(),
                val: "spam".to_owned(),
                cts: "2025-01-01T00:00:00Z".to_owned(),
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
}
