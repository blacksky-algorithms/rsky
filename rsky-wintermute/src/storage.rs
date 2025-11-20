use crate::config::{BLOCK_SIZE, CACHE_SIZE, FSYNC_MS, MEMTABLE_SIZE, WRITE_BUFFER_SIZE};
use crate::types::{BackfillJob, FirehoseEvent, IndexJob, WintermuteError};
use fjall::{Config, Keyspace, PartitionCreateOptions, PartitionHandle};
use std::sync::Arc;

pub struct Storage {
    _db: Arc<Keyspace>,
    firehose_events: PartitionHandle,
    backfill_queue: PartitionHandle,
    index_queue: PartitionHandle,
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

        let backfill_queue = db.open_partition(
            "backfill_queue",
            PartitionCreateOptions::default()
                .max_memtable_size(MEMTABLE_SIZE)
                .block_size(BLOCK_SIZE),
        )?;

        let index_queue = db.open_partition(
            "index_queue",
            PartitionCreateOptions::default()
                .max_memtable_size(MEMTABLE_SIZE)
                .block_size(BLOCK_SIZE),
        )?;

        let cursors = db.open_partition("cursors", PartitionCreateOptions::default())?;

        Ok(Self {
            _db: db,
            firehose_events,
            backfill_queue,
            index_queue,
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
        self.backfill_queue
            .insert(key.as_bytes(), value.as_slice())?;
        Ok(())
    }

    pub fn dequeue_backfill(&self) -> Result<Option<(Vec<u8>, BackfillJob)>, WintermuteError> {
        let mut iter = self.backfill_queue.iter();
        let Some(entry) = iter.next() else {
            return Ok(None);
        };
        let (key, value) = entry?;
        let job = ciborium::from_reader(value.as_ref())
            .map_err(|e| WintermuteError::Serialization(format!("failed to deserialize: {e}")))?;
        Ok(Some((key.to_vec(), job)))
    }

    pub fn remove_backfill(&self, key: &[u8]) -> Result<(), WintermuteError> {
        self.backfill_queue.remove(key)?;
        Ok(())
    }

    pub fn enqueue_index(&self, job: &IndexJob) -> Result<(), WintermuteError> {
        let key = format!(
            "{}:{}",
            job.uri,
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        );
        let mut value = Vec::new();
        ciborium::into_writer(job, &mut value)
            .map_err(|e| WintermuteError::Serialization(format!("failed to serialize job: {e}")))?;
        self.index_queue.insert(key.as_bytes(), value.as_slice())?;
        Ok(())
    }

    pub fn dequeue_index(&self) -> Result<Option<(Vec<u8>, IndexJob)>, WintermuteError> {
        let mut iter = self.index_queue.iter();
        let Some(entry) = iter.next() else {
            return Ok(None);
        };
        let (key, value) = entry?;
        let job = ciborium::from_reader(value.as_ref())
            .map_err(|e| WintermuteError::Serialization(format!("failed to deserialize: {e}")))?;
        Ok(Some((key.to_vec(), job)))
    }

    pub fn remove_index(&self, key: &[u8]) -> Result<(), WintermuteError> {
        self.index_queue.remove(key)?;
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

    pub fn backfill_queue_len(&self) -> Result<usize, WintermuteError> {
        Ok(self.backfill_queue.len()?)
    }

    pub fn index_queue_len(&self) -> Result<usize, WintermuteError> {
        Ok(self.index_queue.len()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BackfillJob, CommitData, FirehoseEvent, IndexJob, WriteAction};
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
    fn test_index_queue() {
        let (storage, _dir) = setup_test_storage();

        let job = IndexJob {
            uri: "at://did:plc:test/app.bsky.feed.post/123".to_owned(),
            cid: "bafytest123".to_owned(),
            action: WriteAction::Create,
            record: Some(serde_json::json!({"test": "data"})),
            indexed_at: "2025-01-01T00:00:00Z".to_owned(),
            rev: "rev123".to_owned(),
        };

        storage.enqueue_index(&job).unwrap();
        let (key, retrieved) = storage.dequeue_index().unwrap().unwrap();

        assert_eq!(retrieved.uri, job.uri);
        assert_eq!(retrieved.cid, job.cid);

        storage.remove_index(&key).unwrap();
        assert!(storage.dequeue_index().unwrap().is_none());
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
}
