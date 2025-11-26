use crate::config::{BLOCK_SIZE, CACHE_SIZE, FSYNC_MS, MEMTABLE_SIZE, WRITE_BUFFER_SIZE};
use crate::types::{BackfillJob, FirehoseEvent, IndexJob, WintermuteError};
use fjall::{Config, Keyspace, PartitionCreateOptions, PartitionHandle};
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
    pub fn new(db_path: Option<std::path::PathBuf>) -> Result<Self, WintermuteError> {
        let path = db_path.unwrap_or_else(|| "wintermute_db".into());
        let db = Config::new(path)
            .cache_size(CACHE_SIZE)
            .max_write_buffer_size(WRITE_BUFFER_SIZE)
            .fsync_ms(FSYNC_MS)
            .open()
            .map_err(|e| WintermuteError::Other(format!("failed to open database: {e}")))?;

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

    pub fn enqueue_backfill(&self, job: &BackfillJob) -> Result<(), WintermuteError> {
        let key = format!(
            "{}:{}",
            job.did,
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
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
    pub fn enqueue_firehose_backfill(&self, job: &IndexJob) -> Result<(), WintermuteError> {
        let key = format!(
            "{}:{}",
            job.uri,
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
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
        };

        storage.enqueue_backfill(&job).unwrap();
        let (key, retrieved) = storage.dequeue_backfill().unwrap().unwrap();

        assert_eq!(retrieved.did, job.did);
        assert_eq!(retrieved.retry_count, job.retry_count);

        storage.remove_backfill(&key).unwrap();
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
}
