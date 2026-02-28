use crate::config::{BLOCK_SIZE, CACHE_SIZE, FSYNC_MS, MEMTABLE_SIZE, WRITE_BUFFER_SIZE};
use crate::types::{BackfillJob, FirehoseEvent, IndexJob, WintermuteError};
use fjall::{Config, Keyspace, PartitionCreateOptions, PartitionHandle};
use heed::types::Bytes;
use heed::{Database as HeedDatabase, Env, EnvOpenOptions};
use std::ops::Bound;
use std::path::PathBuf;
use std::sync::Arc;

/// LMDB max map size: 1TB for `firehose_backfill` queue
const LMDB_MAP_SIZE: usize = 1024 * 1024 * 1024 * 1024;

pub struct Storage {
    #[allow(dead_code)] // Kept for Fjall keyspace - partitions reference it internally
    db: Arc<Keyspace>,
    firehose_events: PartitionHandle,
    repo_backfill: PartitionHandle,
    firehose_live: PartitionHandle,
    label_live: PartitionHandle,
    cursors: PartitionHandle,
    // LMDB for firehose_backfill - eliminates L0 compaction stalls
    lmdb_env: Env,
    firehose_backfill_db: HeedDatabase<Bytes, Bytes>,
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
            MEMTABLE_SIZE / (1024 * 1024)
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

        let label_live = db.open_partition(
            "label_live",
            PartitionCreateOptions::default()
                .max_memtable_size(MEMTABLE_SIZE)
                .block_size(BLOCK_SIZE),
        )?;

        let cursors = db.open_partition("cursors", PartitionCreateOptions::default())?;

        // Open LMDB for firehose_backfill - B+ tree eliminates LSM compaction stalls
        let lmdb_path = path.join("firehose_backfill_lmdb");
        std::fs::create_dir_all(&lmdb_path)
            .map_err(|e| WintermuteError::Other(format!("failed to create LMDB directory: {e}")))?;

        tracing::info!(
            "opening LMDB for firehose_backfill at {} with map_size={}GB",
            lmdb_path.display(),
            LMDB_MAP_SIZE / (1024 * 1024 * 1024)
        );

        let lmdb_env = unsafe {
            EnvOpenOptions::new()
                .map_size(LMDB_MAP_SIZE)
                .max_dbs(1)
                .open(&lmdb_path)
                .map_err(|e| WintermuteError::Other(format!("failed to open LMDB: {e}")))?
        };

        let mut wtxn = lmdb_env
            .write_txn()
            .map_err(|e| WintermuteError::Other(format!("LMDB write txn failed: {e}")))?;
        let firehose_backfill_db: HeedDatabase<Bytes, Bytes> = lmdb_env
            .create_database(&mut wtxn, Some("firehose_backfill"))
            .map_err(|e| WintermuteError::Other(format!("LMDB create database failed: {e}")))?;
        wtxn.commit()
            .map_err(|e| WintermuteError::Other(format!("LMDB commit failed: {e}")))?;

        Ok(Self {
            db,
            firehose_events,
            repo_backfill,
            firehose_live,
            label_live,
            cursors,
            lmdb_env,
            firehose_backfill_db,
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

    /// Enqueue a backfill job with IMMEDIATE priority (timestamp 0)
    /// These items are processed FIRST, before all other priority items
    pub fn enqueue_backfill_immediate(&self, job: &BackfillJob) -> Result<(), WintermuteError> {
        // Key format: "0:0:{did}" - timestamp 0 ensures it sorts first
        let key = format!("0:0:{}", job.did);
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

    // Firehose backfill queue (from backfiller) - uses LMDB for consistent sub-ms iteration
    // Uses key-prefix partitioning for fast parallel dequeue:
    // - Priority items: prefix "0:" (processed first by all workers)
    // - Normal items: prefix "{XX}:" where XX is random hex in range 10-ff
    // Workers claim partitions of the key space for contention-free dequeue

    /// Enqueue a firehose backfill job with normal priority (random prefix 10-ff)
    /// The random prefix enables partitioned dequeue where each worker owns a key range
    pub fn enqueue_firehose_backfill(&self, job: &IndexJob) -> Result<(), WintermuteError> {
        // Key format: "{XX}:{timestamp}:{uri}" where XX is random hex in range 10-ff
        // This distributes items across 240 buckets for parallel dequeue
        let prefix: u8 = rand::random::<u8>().saturating_add(16).max(16); // 16-255 (0x10-0xff)
        let key = format!(
            "{:02x}:{}:{}",
            prefix,
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
            job.uri
        );
        let mut value = Vec::new();
        ciborium::into_writer(job, &mut value)
            .map_err(|e| WintermuteError::Serialization(format!("failed to serialize job: {e}")))?;

        let mut wtxn = self
            .lmdb_env
            .write_txn()
            .map_err(|e| WintermuteError::Other(format!("LMDB write txn failed: {e}")))?;
        self.firehose_backfill_db
            .put(&mut wtxn, key.as_bytes(), &value)
            .map_err(|e| WintermuteError::Other(format!("LMDB put failed: {e}")))?;
        wtxn.commit()
            .map_err(|e| WintermuteError::Other(format!("LMDB commit failed: {e}")))?;

        crate::metrics::INGESTER_FIREHOSE_BACKFILL_LENGTH.inc();
        Ok(())
    }

    /// Batch enqueue multiple firehose backfill jobs in a single transaction
    pub fn enqueue_firehose_backfill_batch(
        &self,
        jobs: &[IndexJob],
    ) -> Result<(), WintermuteError> {
        if jobs.is_empty() {
            return Ok(());
        }

        let mut wtxn = self
            .lmdb_env
            .write_txn()
            .map_err(|e| WintermuteError::Other(format!("LMDB write txn failed: {e}")))?;

        for job in jobs {
            let prefix: u8 = rand::random::<u8>().saturating_add(16).max(16);
            let key = format!(
                "{:02x}:{}:{}",
                prefix,
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
                job.uri
            );
            let mut value = Vec::new();
            ciborium::into_writer(job, &mut value).map_err(|e| {
                WintermuteError::Serialization(format!("failed to serialize job: {e}"))
            })?;
            self.firehose_backfill_db
                .put(&mut wtxn, key.as_bytes(), &value)
                .map_err(|e| WintermuteError::Other(format!("LMDB put failed: {e}")))?;
        }

        wtxn.commit()
            .map_err(|e| WintermuteError::Other(format!("LMDB commit failed: {e}")))?;

        #[allow(clippy::cast_possible_wrap)]
        crate::metrics::INGESTER_FIREHOSE_BACKFILL_LENGTH.add(jobs.len() as i64);
        Ok(())
    }

    /// Enqueue a firehose backfill job with HIGH priority (prefix "0:")
    /// Priority items are indexed BEFORE all normal backfill items
    pub fn enqueue_firehose_backfill_priority(
        &self,
        job: &IndexJob,
    ) -> Result<(), WintermuteError> {
        // Key format: "0:{timestamp}:{uri}" - "0:" prefix for high priority
        // "0:" sorts before "10:" so priority items are always first
        let key = format!(
            "0:{}:{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
            job.uri
        );
        let mut value = Vec::new();
        ciborium::into_writer(job, &mut value)
            .map_err(|e| WintermuteError::Serialization(format!("failed to serialize job: {e}")))?;

        let mut wtxn = self
            .lmdb_env
            .write_txn()
            .map_err(|e| WintermuteError::Other(format!("LMDB write txn failed: {e}")))?;
        self.firehose_backfill_db
            .put(&mut wtxn, key.as_bytes(), &value)
            .map_err(|e| WintermuteError::Other(format!("LMDB put failed: {e}")))?;
        wtxn.commit()
            .map_err(|e| WintermuteError::Other(format!("LMDB commit failed: {e}")))?;

        crate::metrics::INGESTER_FIREHOSE_BACKFILL_LENGTH.inc();
        Ok(())
    }

    pub fn dequeue_firehose_backfill(
        &self,
    ) -> Result<Option<(Vec<u8>, IndexJob)>, WintermuteError> {
        let rtxn = self
            .lmdb_env
            .read_txn()
            .map_err(|e| WintermuteError::Other(format!("LMDB read txn failed: {e}")))?;

        let mut iter = self
            .firehose_backfill_db
            .iter(&rtxn)
            .map_err(|e| WintermuteError::Other(format!("LMDB iter failed: {e}")))?;

        let Some(entry) = iter.next() else {
            return Ok(None);
        };

        let (key, value) =
            entry.map_err(|e| WintermuteError::Other(format!("LMDB iter error: {e}")))?;
        let key_vec = key.to_vec();
        let job: IndexJob = ciborium::from_reader(value)
            .map_err(|e| WintermuteError::Serialization(format!("failed to deserialize: {e}")))?;

        drop(iter);
        drop(rtxn);

        // Remove the item
        let mut wtxn = self
            .lmdb_env
            .write_txn()
            .map_err(|e| WintermuteError::Other(format!("LMDB write txn failed: {e}")))?;
        self.firehose_backfill_db
            .delete(&mut wtxn, &key_vec)
            .map_err(|e| WintermuteError::Other(format!("LMDB delete failed: {e}")))?;
        wtxn.commit()
            .map_err(|e| WintermuteError::Other(format!("LMDB commit failed: {e}")))?;

        crate::metrics::INGESTER_FIREHOSE_BACKFILL_LENGTH.dec();
        Ok(Some((key_vec, job)))
    }

    /// Batch dequeue up to `limit` jobs from `firehose_backfill` in a single iteration
    /// LMDB provides consistent sub-ms latency regardless of queue size
    pub fn dequeue_firehose_backfill_batch(
        &self,
        limit: usize,
    ) -> Result<Vec<(Vec<u8>, IndexJob)>, WintermuteError> {
        let mut results = Vec::with_capacity(limit);

        let rtxn = self
            .lmdb_env
            .read_txn()
            .map_err(|e| WintermuteError::Other(format!("LMDB read txn failed: {e}")))?;

        let iter = self
            .firehose_backfill_db
            .iter(&rtxn)
            .map_err(|e| WintermuteError::Other(format!("LMDB iter failed: {e}")))?;

        for entry in iter.take(limit) {
            let (key, value) =
                entry.map_err(|e| WintermuteError::Other(format!("LMDB iter error: {e}")))?;
            let key_vec = key.to_vec();
            let job: IndexJob = ciborium::from_reader(value).map_err(|e| {
                WintermuteError::Serialization(format!("failed to deserialize: {e}"))
            })?;
            results.push((key_vec, job));
        }

        drop(rtxn);

        // Remove all dequeued items in a single transaction
        if !results.is_empty() {
            let mut wtxn = self
                .lmdb_env
                .write_txn()
                .map_err(|e| WintermuteError::Other(format!("LMDB write txn failed: {e}")))?;
            for (key, _) in &results {
                self.firehose_backfill_db
                    .delete(&mut wtxn, key)
                    .map_err(|e| WintermuteError::Other(format!("LMDB delete failed: {e}")))?;
            }
            wtxn.commit()
                .map_err(|e| WintermuteError::Other(format!("LMDB commit failed: {e}")))?;

            #[allow(clippy::cast_possible_wrap)]
            crate::metrics::INGESTER_FIREHOSE_BACKFILL_LENGTH.sub(results.len() as i64);
        }

        Ok(results)
    }

    /// Partitioned dequeue for parallel workers - each worker owns a slice of the key space
    ///
    /// Key space partitioning:
    /// - Priority items (prefix "0:") are checked first by ALL workers
    /// - Normal items (prefix "10"-"ff") are partitioned among workers
    ///
    /// With N workers, worker i owns prefixes in range [start, end) where:
    /// - start = 0x10 + (i * 240 / N)
    /// - end = 0x10 + ((i + 1) * 240 / N)
    ///
    /// This eliminates contention since each worker reads from its own partition.
    /// LMDB provides consistent sub-ms latency for all range scans.
    pub fn dequeue_firehose_backfill_partitioned(
        &self,
        worker_id: usize,
        num_workers: usize,
        limit: usize,
    ) -> Result<Vec<(Vec<u8>, IndexJob)>, WintermuteError> {
        let mut results = Vec::with_capacity(limit);

        let rtxn = self
            .lmdb_env
            .read_txn()
            .map_err(|e| WintermuteError::Other(format!("LMDB read txn failed: {e}")))?;

        // First, check for priority items (all workers can grab these)
        // Priority prefix "0:" sorts before "10:"
        let priority_start: &[u8] = b"0:";
        let priority_end: &[u8] = b"0;";
        let priority_bounds = (
            Bound::Included(priority_start),
            Bound::Excluded(priority_end),
        );
        let priority_range = self
            .firehose_backfill_db
            .range(&rtxn, &priority_bounds)
            .map_err(|e| WintermuteError::Other(format!("LMDB range failed: {e}")))?;

        for entry in priority_range {
            if results.len() >= limit {
                break;
            }
            let (key, value) =
                entry.map_err(|e| WintermuteError::Other(format!("LMDB iter error: {e}")))?;
            let key_vec = key.to_vec();
            let job: IndexJob = ciborium::from_reader(value).map_err(|e| {
                WintermuteError::Serialization(format!("failed to deserialize: {e}"))
            })?;
            results.push((key_vec, job));
        }

        // If we got enough priority items, batch remove and return early
        if results.len() >= limit {
            drop(rtxn);
            self.batch_remove_firehose_backfill(&results)?;
            return Ok(results);
        }

        // Handle legacy items with "1:" prefix (from before partitioning was added)
        let legacy_start: &[u8] = b"1:";
        let legacy_end: &[u8] = b"1;";
        let legacy_bounds = (Bound::Included(legacy_start), Bound::Excluded(legacy_end));
        let legacy_range = self
            .firehose_backfill_db
            .range(&rtxn, &legacy_bounds)
            .map_err(|e| WintermuteError::Other(format!("LMDB range failed: {e}")))?;

        for entry in legacy_range {
            if results.len() >= limit {
                break;
            }
            let (key, value) =
                entry.map_err(|e| WintermuteError::Other(format!("LMDB iter error: {e}")))?;
            let key_vec = key.to_vec();
            let job: IndexJob = ciborium::from_reader(value).map_err(|e| {
                WintermuteError::Serialization(format!("failed to deserialize: {e}"))
            })?;
            results.push((key_vec, job));
        }

        // If we got enough items from legacy queue, batch remove and return early
        if results.len() >= limit {
            drop(rtxn);
            self.batch_remove_firehose_backfill(&results)?;
            return Ok(results);
        }

        // Calculate this worker's partition range (prefixes 0x10 to 0xff = 240 values)
        let partition_size = 240 / num_workers;
        let start_prefix = 0x10 + (worker_id * partition_size);
        let end_prefix = if worker_id == num_workers - 1 {
            0x100 // Last worker gets remainder
        } else {
            0x10 + ((worker_id + 1) * partition_size)
        };

        // Build range keys
        let start_key = format!("{start_prefix:02x}:");
        let end_key_string;
        let end_key: &[u8] = if worker_id == num_workers - 1 {
            // Last worker reads to end of keyspace
            &[0xff, 0xff]
        } else {
            end_key_string = format!("{end_prefix:02x}:");
            end_key_string.as_bytes()
        };

        // Iterate over this worker's partition using LMDB range
        let partition_bounds = (
            Bound::Included(start_key.as_bytes()),
            Bound::Excluded(end_key),
        );
        let partition_range = self
            .firehose_backfill_db
            .range(&rtxn, &partition_bounds)
            .map_err(|e| WintermuteError::Other(format!("LMDB range failed: {e}")))?;

        for entry in partition_range {
            if results.len() >= limit {
                break;
            }
            let (key, value) =
                entry.map_err(|e| WintermuteError::Other(format!("LMDB iter error: {e}")))?;
            let key_vec = key.to_vec();
            let job: IndexJob = ciborium::from_reader(value).map_err(|e| {
                WintermuteError::Serialization(format!("failed to deserialize: {e}"))
            })?;
            results.push((key_vec, job));
        }

        drop(rtxn);

        // Batch remove all dequeued items
        self.batch_remove_firehose_backfill(&results)?;

        Ok(results)
    }

    /// Helper to batch remove items from `firehose_backfill` queue using LMDB
    fn batch_remove_firehose_backfill(
        &self,
        items: &[(Vec<u8>, IndexJob)],
    ) -> Result<(), WintermuteError> {
        if items.is_empty() {
            return Ok(());
        }

        let mut wtxn = self
            .lmdb_env
            .write_txn()
            .map_err(|e| WintermuteError::Other(format!("LMDB write txn failed: {e}")))?;

        for (key, _) in items {
            self.firehose_backfill_db
                .delete(&mut wtxn, key)
                .map_err(|e| WintermuteError::Other(format!("LMDB delete failed: {e}")))?;
        }

        wtxn.commit()
            .map_err(|e| WintermuteError::Other(format!("LMDB commit failed: {e}")))?;

        #[allow(clippy::cast_possible_wrap)]
        crate::metrics::INGESTER_FIREHOSE_BACKFILL_LENGTH.sub(items.len() as i64);
        Ok(())
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
        let rtxn = self
            .lmdb_env
            .read_txn()
            .map_err(|e| WintermuteError::Other(format!("LMDB read txn failed: {e}")))?;
        let len = self
            .firehose_backfill_db
            .len(&rtxn)
            .map_err(|e| WintermuteError::Other(format!("LMDB len failed: {e}")))?;
        #[allow(clippy::cast_possible_truncation)]
        Ok(len as usize)
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

    /// Remove all entries for a specific DID from `repo_backfill`
    /// Returns the number of entries removed
    pub fn remove_backfill_by_did(&self, did: &str) -> Result<usize, WintermuteError> {
        let mut removed = 0;
        let mut keys_to_remove = Vec::new();

        for entry in self.repo_backfill.iter() {
            let (key, value) = entry?;
            let job: BackfillJob = ciborium::from_reader(value.as_ref())
                .map_err(|e| WintermuteError::Serialization(format!("deserialize failed: {e}")))?;
            if job.did == did {
                keys_to_remove.push(key.to_vec());
            }
        }

        for key in keys_to_remove {
            self.repo_backfill.remove(&key)?;
            crate::metrics::INGESTER_REPO_BACKFILL_LENGTH.dec();
            removed += 1;
        }

        Ok(removed)
    }

    /// Clear all items from `repo_backfill`
    pub fn clear_repo_backfill(&self) -> Result<(), WintermuteError> {
        let mut keys_to_remove = Vec::new();

        for entry in self.repo_backfill.iter() {
            let (key, _) = entry?;
            keys_to_remove.push(key.to_vec());
        }

        for key in keys_to_remove {
            self.repo_backfill.remove(&key)?;
            crate::metrics::INGESTER_REPO_BACKFILL_LENGTH.dec();
        }

        Ok(())
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
    fn test_firehose_backfill_partitioned_dequeue() {
        let (storage, _dir) = setup_test_storage();

        // Enqueue 20 jobs with random prefixes
        for i in 0..20 {
            let job = IndexJob {
                uri: format!("at://did:plc:test/app.bsky.feed.post/part{i}"),
                cid: format!("bafypart{i}"),
                action: WriteAction::Create,
                record: Some(serde_json::json!({"index": i})),
                indexed_at: "2025-01-01T00:00:00Z".to_owned(),
                rev: format!("rev{i}"),
            };
            storage.enqueue_firehose_backfill(&job).unwrap();
        }

        // Use 2 workers to partition the key space
        let num_workers = 2;

        // Worker 0 dequeues from its partition
        let worker0_batch1 = storage
            .dequeue_firehose_backfill_partitioned(0, num_workers, 10)
            .unwrap();

        // Worker 1 dequeues from its partition
        let worker1_batch1 = storage
            .dequeue_firehose_backfill_partitioned(1, num_workers, 10)
            .unwrap();

        // Both workers should get items (unless distribution is very unlucky)
        // With 20 items across 240 buckets, expect at least some items per partition
        let total_first_round = worker0_batch1.len() + worker1_batch1.len();

        // Continue dequeuing until empty
        let worker0_batch2 = storage
            .dequeue_firehose_backfill_partitioned(0, num_workers, 10)
            .unwrap();
        let worker1_batch2 = storage
            .dequeue_firehose_backfill_partitioned(1, num_workers, 10)
            .unwrap();

        let total_second_round = worker0_batch2.len() + worker1_batch2.len();

        // All 20 items should have been dequeued
        assert_eq!(
            total_first_round + total_second_round,
            20,
            "all 20 items should be dequeued across partitions"
        );

        // Queue should now be empty
        assert!(storage.dequeue_firehose_backfill().unwrap().is_none());
    }

    #[test]
    fn test_firehose_backfill_partitioned_priority() {
        let (storage, _dir) = setup_test_storage();

        // Enqueue normal items
        for i in 0..5 {
            let job = IndexJob {
                uri: format!("at://did:plc:normal/app.bsky.feed.post/{i}"),
                cid: format!("bafynorm{i}"),
                action: WriteAction::Create,
                record: Some(serde_json::json!({"type": "normal"})),
                indexed_at: "2025-01-01T00:00:00Z".to_owned(),
                rev: format!("rev{i}"),
            };
            storage.enqueue_firehose_backfill(&job).unwrap();
        }

        // Enqueue priority items
        for i in 0..3 {
            let job = IndexJob {
                uri: format!("at://did:plc:priority/app.bsky.feed.post/{i}"),
                cid: format!("bafypri{i}"),
                action: WriteAction::Create,
                record: Some(serde_json::json!({"type": "priority"})),
                indexed_at: "2025-01-01T00:00:00Z".to_owned(),
                rev: format!("rev_pri{i}"),
            };
            storage.enqueue_firehose_backfill_priority(&job).unwrap();
        }

        // Worker 0 should get priority items first
        let batch = storage
            .dequeue_firehose_backfill_partitioned(0, 2, 10)
            .unwrap();

        // Priority items should come first (they all have "priority" in the URI)
        let priority_count = batch
            .iter()
            .take(3)
            .filter(|(_, job)| job.uri.contains("priority"))
            .count();
        assert_eq!(
            priority_count, 3,
            "all 3 priority items should be at the start"
        );
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

    #[test]
    fn test_remove_backfill_by_did() {
        let (storage, _dir) = setup_test_storage();

        // Enqueue multiple DIDs
        let job1 = BackfillJob {
            did: "did:plc:target".to_owned(),
            retry_count: 0,
            priority: false,
        };
        let job2 = BackfillJob {
            did: "did:plc:other".to_owned(),
            retry_count: 0,
            priority: false,
        };
        let job3 = BackfillJob {
            did: "did:plc:target".to_owned(),
            retry_count: 1,
            priority: true,
        };

        storage.enqueue_backfill(&job1).unwrap();
        storage.enqueue_backfill(&job2).unwrap();
        storage.enqueue_backfill_priority(&job3).unwrap();

        assert_eq!(storage.repo_backfill_len().unwrap(), 3);

        // Remove all entries for target DID
        let removed = storage.remove_backfill_by_did("did:plc:target").unwrap();
        assert_eq!(removed, 2, "should remove both entries for target");

        // Only other DID should remain
        assert_eq!(storage.repo_backfill_len().unwrap(), 1);
        let (_, remaining) = storage.dequeue_backfill().unwrap().unwrap();
        assert_eq!(remaining.did, "did:plc:other");
    }

    #[test]
    fn test_remove_backfill_by_did_not_found() {
        let (storage, _dir) = setup_test_storage();

        let job = BackfillJob {
            did: "did:plc:existing".to_owned(),
            retry_count: 0,
            priority: false,
        };
        storage.enqueue_backfill(&job).unwrap();

        let removed = storage
            .remove_backfill_by_did("did:plc:nonexistent")
            .unwrap();
        assert_eq!(removed, 0, "should return 0 when DID not found");
        assert_eq!(storage.repo_backfill_len().unwrap(), 1);
    }

    #[test]
    fn test_clear_repo_backfill() {
        let (storage, _dir) = setup_test_storage();

        // Enqueue several items
        for i in 0..10 {
            let job = BackfillJob {
                did: format!("did:plc:test{i}"),
                retry_count: 0,
                priority: i % 2 == 0,
            };
            if i % 2 == 0 {
                storage.enqueue_backfill_priority(&job).unwrap();
            } else {
                storage.enqueue_backfill(&job).unwrap();
            }
        }

        assert_eq!(storage.repo_backfill_len().unwrap(), 10);

        storage.clear_repo_backfill().unwrap();

        assert_eq!(storage.repo_backfill_len().unwrap(), 0);
        assert!(storage.dequeue_backfill().unwrap().is_none());
    }

    #[test]
    fn test_clear_empty_repo_backfill() {
        let (storage, _dir) = setup_test_storage();

        assert_eq!(storage.repo_backfill_len().unwrap(), 0);
        storage.clear_repo_backfill().unwrap();
        assert_eq!(storage.repo_backfill_len().unwrap(), 0);
    }
}
