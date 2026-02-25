mod bulk;
mod tests;

use crate::SHUTDOWN;
use crate::config::{
    DB_POOL_SIZE, HANDLE_PRIORITY_WINDOW, HANDLE_REINDEX_INTERVAL_INVALID,
    HANDLE_REINDEX_INTERVAL_VALID, HANDLE_RESOLUTION_BATCH_SIZE, HANDLE_RESOLUTION_CONCURRENCY,
    IDENTITY_RESOLVER_TIMEOUT, INLINE_CONCURRENCY, WORKERS_INDEXER,
};
use crate::config::{INDEXER_BATCH_SIZE, INDEXER_BATCH_WORKERS};
use crate::storage::Storage;
#[cfg(test)]
use crate::types::LabelEvent;
use crate::types::{IndexJob, WintermuteError, WriteAction};
use dashmap::DashMap;
use deadpool_postgres::{Config, ManagerConfig, Pool, RecyclingMethod, Runtime};
use futures::FutureExt;
use futures::stream::{FuturesUnordered, StreamExt};
use rsky_identity::IdResolver;
use rsky_identity::types::IdentityResolverOpts;
use rsky_syntax::aturi::AtUri;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::{Mutex, Semaphore};
use tokio_postgres::NoTls;

// Global semaphore to serialize like inserts across all workers.
// The like table has a 133GB index that causes severe contention when
// multiple workers hit it simultaneously. Serializing like access prevents
// 10+ second delays from index cache misses.
static LIKE_INSERT_SEMAPHORE: std::sync::LazyLock<Semaphore> =
    std::sync::LazyLock::new(|| Semaphore::new(1));

// Cache of actor DIDs known to exist in the DB, avoiding redundant INSERT ON CONFLICT DO NOTHING.
// Bounded to 2M entries to prevent unbounded memory growth.
static ACTOR_CACHE: std::sync::LazyLock<DashMap<String, ()>> =
    std::sync::LazyLock::new(DashMap::new);
const ACTOR_CACHE_MAX_SIZE: usize = 2_000_000;

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub enum QueueSource {
    FirehoseLive,
    FirehoseBackfill,
    LabelLive, // Future use for label stream processing
}

// Type aliases to reduce complexity
#[cfg(test)]
type IndexJobWithMetadata = (Vec<u8>, IndexJob, QueueSource);
#[cfg(test)]
type LabelJobWithMetadata = (Vec<u8>, LabelEvent);
type JobTaskResult = (Vec<u8>, QueueSource, Result<(), WintermuteError>);
type JobTaskJoinResult = Result<JobTaskResult, tokio::task::JoinError>;
type JobTaskHandle = tokio::task::JoinHandle<JobTaskResult>;
type LabelTaskResult = (Vec<u8>, Result<(), WintermuteError>);
type LabelTaskHandle = tokio::task::JoinHandle<LabelTaskResult>;

fn sanitize_text(s: &str) -> String {
    s.replace('\0', "")
}

fn sanitize_opt(s: Option<&str>) -> Option<String> {
    s.map(|v| v.replace('\0', ""))
}

pub struct IndexerManager {
    workers: usize,
    storage: Arc<Storage>,
    pool_live: Pool,
    pool_backfill: Pool,
    pool_labels: Pool,
    #[cfg_attr(not(test), allow(dead_code))]
    semaphore_backfill: Arc<Semaphore>,
    id_resolver: Arc<Mutex<IdResolver>>,
}

/// Parsed job data for batch processing
struct ParsedJob<'a> {
    key: &'a Vec<u8>,
    job: &'a IndexJob,
    uri: AtUri,
    did: String,
    collection: String,
    rkey: String,
}

impl IndexerManager {
    pub fn new(storage: Arc<Storage>, database_url: &str) -> Result<Self, WintermuteError> {
        let pool_size = *DB_POOL_SIZE;
        // Create separate pools for each stream to prevent starvation
        // Backfill gets 50% of connections since it's the main bottleneck
        let backfill_pool_size = pool_size / 2;
        let live_pool_size = pool_size / 4;
        let labels_pool_size = pool_size / 4;

        tracing::info!(
            "indexer DB pools: live={}, backfill={}, labels={}",
            live_pool_size,
            backfill_pool_size,
            labels_pool_size
        );

        let pool_live = Self::create_pool(database_url, live_pool_size.max(5))?;
        let pool_backfill = Self::create_pool(database_url, backfill_pool_size.max(10))?;
        let pool_labels = Self::create_pool(database_url, labels_pool_size.max(5))?;

        let id_resolver = IdResolver::new(IdentityResolverOpts {
            timeout: Some(IDENTITY_RESOLVER_TIMEOUT),
            plc_url: None,
            did_cache: None,
            backup_nameservers: None,
        });

        let workers = *WORKERS_INDEXER;
        Ok(Self {
            workers,
            storage,
            pool_live,
            pool_backfill,
            pool_labels,
            // Only backfill gets semaphore; firehose_live and label_live are unbounded
            semaphore_backfill: Arc::new(Semaphore::new(workers)),
            id_resolver: Arc::new(Mutex::new(id_resolver)),
        })
    }

    fn create_pool(database_url: &str, size: usize) -> Result<Pool, WintermuteError> {
        let mut pg_config = Config::new();
        pg_config.url = Some(database_url.to_owned());
        pg_config.manager = Some(ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        });
        pg_config.pool = Some(deadpool_postgres::PoolConfig::new(size));

        pg_config
            .create_pool(Some(Runtime::Tokio1), NoTls)
            .map_err(|e| WintermuteError::Other(format!("pool creation failed: {e}")))
    }

    pub fn run(self) -> Result<(), WintermuteError> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(self.workers)
            .enable_all()
            .build()
            .map_err(|e| WintermuteError::Other(format!("failed to create runtime: {e}")))?;

        // Wrap self in Arc for sharing across spawned tasks
        let manager = Arc::new(self);

        rt.block_on(async {
            tracing::info!("indexer starting 4 parallel processors");

            // Spawn each processor as independent task for true parallelism
            let live_handle = {
                let mgr = manager.clone();
                tokio::spawn(async move { mgr.process_firehose_live_loop().await })
            };

            let backfill_handle = {
                let mgr = manager.clone();
                tokio::spawn(async move { mgr.process_firehose_backfill_loop().await })
            };

            let labels_handle = {
                let mgr = manager.clone();
                tokio::spawn(async move { mgr.process_labels_loop().await })
            };

            let handles_handle = {
                let mgr = manager.clone();
                tokio::spawn(async move { mgr.process_handle_resolution_loop().await })
            };

            // Wait for all to complete (they run until shutdown)
            let _results =
                tokio::join!(live_handle, backfill_handle, labels_handle, handles_handle);
        });

        Ok(())
    }

    async fn process_handle_resolution_loop(&self) {
        type HandleFuture =
            std::pin::Pin<Box<dyn std::future::Future<Output = Option<bool>> + Send>>;

        let max_concurrent = *HANDLE_RESOLUTION_CONCURRENCY;
        tracing::info!(
            "handle resolution processor started (concurrency={max_concurrent}, batch={})",
            *HANDLE_RESOLUTION_BATCH_SIZE
        );
        let mut batch_count = 0u64;
        let mut total_resolved = 0u64;

        loop {
            if SHUTDOWN.load(Ordering::Relaxed) {
                tracing::info!("shutdown requested for handle resolution processor");
                break;
            }

            // Query actors with NULL handle or stale indexedAt
            let dids_to_resolve = match self.get_actors_needing_handle_resolution().await {
                Ok(dids) => dids,
                Err(e) => {
                    tracing::warn!("failed to get actors needing handle resolution: {e}");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };

            if dids_to_resolve.is_empty() {
                // No work to do, sleep longer
                tokio::time::sleep(Duration::from_secs(10)).await;
                continue;
            }

            batch_count += 1;
            let batch_size = dids_to_resolve.len();

            let timestamp = chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string();
            let id_resolver = Arc::clone(&self.id_resolver);
            let pool = self.pool_labels.clone();

            // Process handles in parallel using FuturesUnordered with boxed futures
            let mut in_flight: FuturesUnordered<HandleFuture> = FuturesUnordered::new();
            let mut pending_dids = dids_to_resolve.into_iter();
            let mut resolved_count = 0usize;

            // Helper to create boxed future
            let make_future = |pool: Pool,
                               id_resolver: Arc<Mutex<IdResolver>>,
                               did: String,
                               timestamp: String|
             -> HandleFuture {
                Box::pin(async move {
                    let client = pool.get().await.ok()?;
                    Self::index_handle(&client, &id_resolver, &did, &timestamp, false)
                        .await
                        .ok()
                })
            };

            // Seed initial batch of concurrent tasks
            for did in pending_dids.by_ref().take(max_concurrent) {
                in_flight.push(make_future(
                    pool.clone(),
                    Arc::clone(&id_resolver),
                    did,
                    timestamp.clone(),
                ));
            }

            // Process results and spawn new tasks as slots free up
            while let Some(result) = in_flight.next().await {
                if result == Some(true) {
                    resolved_count += 1;
                }

                // Spawn next task if there are more DIDs
                if let Some(did) = pending_dids.next() {
                    in_flight.push(make_future(
                        pool.clone(),
                        Arc::clone(&id_resolver),
                        did,
                        timestamp.clone(),
                    ));
                }
            }

            total_resolved += resolved_count as u64;

            // Log progress every 10 batches or when handles are resolved
            if batch_count % 10 == 1 || resolved_count > 0 {
                tracing::info!(
                    "handle resolution batch {batch_count}: {resolved_count}/{batch_size} resolved (total: {total_resolved})"
                );
            }

            // Small delay between batches to avoid overwhelming the system
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn get_actors_needing_handle_resolution(&self) -> Result<Vec<String>, WintermuteError> {
        let client = self.pool_labels.get().await?;
        let batch_size = i64::try_from(*HANDLE_RESOLUTION_BATCH_SIZE).unwrap_or(500);

        // Get actors with NULL handle or stale indexedAt
        // Priority order:
        // 1. Recently-indexed actors with NULL handle (within priority window) - newest first
        // 2. Older actors with NULL handle - oldest first
        // 3. Actors with stale valid handles - oldest first
        let stale_threshold_invalid = (chrono::Utc::now()
            - chrono::Duration::from_std(HANDLE_REINDEX_INTERVAL_INVALID).unwrap_or_default())
        .to_rfc3339();
        let stale_threshold_valid = (chrono::Utc::now()
            - chrono::Duration::from_std(HANDLE_REINDEX_INTERVAL_VALID).unwrap_or_default())
        .to_rfc3339();
        let priority_window = (chrono::Utc::now()
            - chrono::Duration::from_std(HANDLE_PRIORITY_WINDOW).unwrap_or_default())
        .to_rfc3339();

        // Query with priority: recent NULL handles first (newest), then older NULL handles (oldest), then stale valid handles
        let rows = client
            .query(
                "SELECT did FROM actor
                 WHERE (handle IS NULL AND \"indexedAt\" < $1)
                    OR (handle IS NOT NULL AND \"indexedAt\" < $2)
                 ORDER BY
                   CASE
                     WHEN handle IS NULL AND \"indexedAt\" >= $3 THEN 0  -- Recent NULL: highest priority
                     WHEN handle IS NULL THEN 1                          -- Older NULL: second priority
                     ELSE 2                                              -- Stale valid: lowest priority
                   END,
                   CASE
                     WHEN handle IS NULL AND \"indexedAt\" >= $3 THEN \"indexedAt\"  -- Recent: newest first
                     ELSE NULL
                   END DESC NULLS LAST,
                   \"indexedAt\" ASC  -- Older entries: oldest first
                 LIMIT $4",
                &[
                    &stale_threshold_invalid,
                    &stale_threshold_valid,
                    &priority_window,
                    &batch_size,
                ],
            )
            .await?;

        let dids: Vec<String> = rows.iter().map(|row| row.get("did")).collect();
        Ok(dids)
    }

    async fn process_firehose_live_loop(&self) {
        let max_concurrent = *INLINE_CONCURRENCY;

        tracing::info!(max_concurrent, "firehose_live processor started");
        let mut in_flight: FuturesUnordered<JobTaskHandle> = FuturesUnordered::new();
        let mut processed_count = 0u64;
        let mut last_processed_count = 0u64;
        let mut last_log = std::time::Instant::now();

        loop {
            if SHUTDOWN.load(Ordering::Relaxed) {
                tracing::info!(
                    "shutdown requested for firehose_live processor, draining {} in-flight tasks",
                    in_flight.len()
                );
                // Drain with timeout to avoid hanging forever
                let drain_start = std::time::Instant::now();
                while !in_flight.is_empty() && drain_start.elapsed() < Duration::from_secs(5) {
                    tokio::select! {
                        Some(result) = in_flight.next() => {
                            self.handle_single_job_result(result);
                        }
                        () = tokio::time::sleep(Duration::from_millis(100)) => {}
                    }
                }
                if !in_flight.is_empty() {
                    tracing::warn!(
                        "firehose_live: {} tasks still in-flight after drain timeout",
                        in_flight.len()
                    );
                }
                break;
            }

            self.update_queue_metrics();

            // First, drain all completed tasks (non-blocking)
            while let Some(result) = in_flight.next().now_or_never().flatten() {
                self.handle_single_job_result(result);
                processed_count += 1;
            }

            // Dequeue jobs up to max concurrent limit (matches pool capacity)
            while in_flight.len() < max_concurrent {
                if SHUTDOWN.load(Ordering::Relaxed) {
                    break;
                }
                match self.storage.dequeue_firehose_live() {
                    Ok(Some((key, job))) => {
                        let pool = self.pool_live.clone();
                        let task = tokio::spawn(async move {
                            let result = Self::process_job(&pool, &job).await;
                            (key, QueueSource::FirehoseLive, result)
                        });
                        in_flight.push(task);
                    }
                    Ok(None) => break,
                    Err(e) => {
                        tracing::error!("failed to dequeue firehose_live job: {e}");
                        break;
                    }
                }
            }

            // Wait for at least one task to complete before next iteration
            if in_flight.is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            } else if in_flight.len() >= max_concurrent {
                // At capacity - wait for one to complete
                if let Some(result) = in_flight.next().await {
                    self.handle_single_job_result(result);
                    processed_count += 1;
                }
            } else {
                tokio::task::yield_now().await;
            }

            // Log progress periodically - only if work was done
            if last_log.elapsed() > Duration::from_secs(5) {
                let elapsed_secs = last_log.elapsed().as_secs_f64();
                #[allow(clippy::cast_precision_loss)]
                let rate = (processed_count - last_processed_count) as f64 / elapsed_secs;
                if processed_count > last_processed_count {
                    tracing::info!(
                        "firehose_live: {} indexed ({:.1}/s), {} in_flight",
                        processed_count - last_processed_count,
                        rate,
                        in_flight.len()
                    );
                }
                last_processed_count = processed_count;
                last_log = std::time::Instant::now();
            }
        }
    }

    async fn process_firehose_backfill_loop(&self) {
        use std::sync::atomic::AtomicU64;

        let batch_size = *INDEXER_BATCH_SIZE;
        let num_workers = *INDEXER_BATCH_WORKERS;

        tracing::info!(
            "firehose_backfill processor started (workers: {}, batch size: {})",
            num_workers,
            batch_size
        );

        // Shared counter for aggregate statistics across all workers
        let processed_total = Arc::new(AtomicU64::new(0));

        // Spawn worker tasks
        let mut worker_handles = Vec::with_capacity(num_workers);
        for worker_id in 0..num_workers {
            let storage = Arc::clone(&self.storage);
            let pool = self.pool_backfill.clone();
            let processed = Arc::clone(&processed_total);

            let handle = tokio::spawn(async move {
                Self::backfill_worker_loop(worker_id, storage, pool, batch_size, processed).await;
            });
            worker_handles.push(handle);
        }

        // Logging task - reports aggregate progress
        let storage_for_logging = Arc::clone(&self.storage);
        let processed_for_logging = Arc::clone(&processed_total);
        let logging_handle = tokio::spawn(async move {
            let mut last_count = 0u64;
            let mut last_log = std::time::Instant::now();

            loop {
                if SHUTDOWN.load(Ordering::Relaxed) {
                    break;
                }

                tokio::time::sleep(Duration::from_secs(5)).await;

                let current = processed_for_logging.load(Ordering::Relaxed);
                let elapsed_secs = last_log.elapsed().as_secs_f64();
                #[allow(clippy::cast_precision_loss)]
                let rate = (current - last_count) as f64 / elapsed_secs;
                let queue_len = storage_for_logging.firehose_backfill_len().unwrap_or(0);

                tracing::info!(
                    "firehose_backfill: {} indexed ({:.1}/s), {} queued",
                    current - last_count,
                    rate,
                    queue_len
                );

                last_count = current;
                last_log = std::time::Instant::now();
            }
        });

        // Wait for shutdown
        for handle in worker_handles {
            let _result = handle.await;
        }
        logging_handle.abort();

        tracing::info!(
            "firehose_backfill processor stopped, total processed: {}",
            processed_total.load(Ordering::Relaxed)
        );
    }

    async fn backfill_worker_loop(
        worker_id: usize,
        storage: Arc<Storage>,
        pool: Pool,
        batch_size: usize,
        processed_count: Arc<std::sync::atomic::AtomicU64>,
    ) {
        use std::time::Instant;

        tracing::debug!("firehose_backfill worker {} started", worker_id);

        loop {
            if SHUTDOWN.load(Ordering::Relaxed) {
                tracing::debug!("firehose_backfill worker {} shutting down", worker_id);
                break;
            }

            // Dequeue a batch of jobs using partitioned dequeue for faster access
            let dequeue_start = Instant::now();
            let num_workers = *INDEXER_BATCH_WORKERS;
            let jobs = match storage.dequeue_firehose_backfill_partitioned(
                worker_id,
                num_workers,
                batch_size,
            ) {
                Ok(jobs) => jobs,
                Err(e) => {
                    // Check if Fjall is poisoned - trigger shutdown for recovery
                    if e.is_storage_corrupted() {
                        tracing::error!(
                            "worker {}: Fjall storage corrupted, triggering shutdown for recovery: {e}",
                            worker_id
                        );
                        SHUTDOWN.store(true, Ordering::Relaxed);
                        break;
                    }
                    tracing::error!(
                        "worker {}: failed to dequeue firehose_backfill jobs: {e}",
                        worker_id
                    );
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            };
            let dequeue_ms = dequeue_start.elapsed().as_millis();

            if jobs.is_empty() {
                // No jobs available, wait briefly before retrying
                tokio::time::sleep(Duration::from_millis(10)).await;
                continue;
            }

            let batch_len = jobs.len();

            // Process the entire batch with batch INSERT statements
            let process_start = Instant::now();
            let results = Self::process_jobs_batch(&pool, &jobs).await;
            let process_ms = process_start.elapsed().as_millis();

            // Handle results - remove jobs from queue
            let remove_start = Instant::now();
            for (key, result) in results {
                if let Err(e) = &result {
                    tracing::error!("worker {}: firehose_backfill job failed: {e}", worker_id);
                }
                if let Err(e) = storage.remove_firehose_backfill(&key) {
                    if e.is_storage_corrupted() {
                        tracing::error!(
                            "worker {}: Fjall storage corrupted during remove, triggering shutdown: {e}",
                            worker_id
                        );
                        SHUTDOWN.store(true, Ordering::Relaxed);
                        return;
                    }
                    tracing::error!(
                        "worker {}: failed to remove firehose_backfill job: {e}",
                        worker_id
                    );
                }
            }
            let remove_ms = remove_start.elapsed().as_millis();

            tracing::info!(
                "worker {}: dequeue={}ms, process={}ms, remove={}ms, batch={}",
                worker_id,
                dequeue_ms,
                process_ms,
                remove_ms,
                batch_len
            );

            processed_count.fetch_add(batch_len as u64, Ordering::Relaxed);
        }
    }

    async fn process_labels_loop(&self) {
        let max_concurrent = *INLINE_CONCURRENCY;

        tracing::info!(max_concurrent, "label_live processor started");
        let mut in_flight: FuturesUnordered<LabelTaskHandle> = FuturesUnordered::new();
        let mut processed_count = 0u64;
        let mut last_processed_count = 0u64;
        let mut last_log = std::time::Instant::now();

        loop {
            if SHUTDOWN.load(Ordering::Relaxed) {
                tracing::info!(
                    "shutdown requested for label_live processor, draining {} in-flight tasks",
                    in_flight.len()
                );
                // Drain with timeout to avoid hanging forever
                let drain_start = std::time::Instant::now();
                while !in_flight.is_empty() && drain_start.elapsed() < Duration::from_secs(5) {
                    tokio::select! {
                        Some(result) = in_flight.next() => {
                            self.handle_single_label_result(result);
                        }
                        () = tokio::time::sleep(Duration::from_millis(100)) => {}
                    }
                }
                if !in_flight.is_empty() {
                    tracing::warn!(
                        "label_live: {} tasks still in-flight after drain timeout",
                        in_flight.len()
                    );
                }
                break;
            }

            // First, drain all completed tasks (non-blocking)
            while let Some(result) = in_flight.next().now_or_never().flatten() {
                self.handle_single_label_result(result);
                processed_count += 1;
            }

            // Dequeue jobs up to max concurrent limit (matches pool capacity)
            while in_flight.len() < max_concurrent {
                if SHUTDOWN.load(Ordering::Relaxed) {
                    break;
                }
                match self.storage.dequeue_label_live() {
                    Ok(Some((key, label_event))) => {
                        let pool = self.pool_labels.clone();
                        let task = tokio::spawn(async move {
                            let result = Self::process_label_event(&pool, &label_event).await;
                            (key, result)
                        });
                        in_flight.push(task);
                    }
                    Ok(None) => break,
                    Err(e) => {
                        tracing::error!("failed to dequeue label_live job: {e}");
                        break;
                    }
                }
            }

            // Wait for at least one task to complete before next iteration
            if in_flight.is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            } else if in_flight.len() >= max_concurrent {
                // At capacity - wait for one to complete
                if let Some(result) = in_flight.next().await {
                    self.handle_single_label_result(result);
                    processed_count += 1;
                }
            } else {
                tokio::task::yield_now().await;
            }

            // Log progress periodically - only if work was done
            if last_log.elapsed() > Duration::from_secs(5) {
                let elapsed_secs = last_log.elapsed().as_secs_f64();
                #[allow(clippy::cast_precision_loss)]
                let rate = (processed_count - last_processed_count) as f64 / elapsed_secs;
                if processed_count > last_processed_count {
                    tracing::info!(
                        "label_live: {} indexed ({:.1}/s), {} in_flight",
                        processed_count - last_processed_count,
                        rate,
                        in_flight.len()
                    );
                }
                last_processed_count = processed_count;
                last_log = std::time::Instant::now();
            }
        }
    }

    fn handle_single_label_result(&self, result: Result<LabelTaskResult, tokio::task::JoinError>) {
        match result {
            Ok((key, Ok(()))) => {
                if let Err(e) = self.storage.remove_label_live(&key) {
                    tracing::error!("failed to remove label from queue: {e}");
                }
            }
            Ok((_, Err(e))) => {
                crate::metrics::INDEXER_RECORDS_FAILED_TOTAL.inc();
                tracing::error!("label job failed: {e}");
            }
            Err(e) => {
                tracing::error!("label task panicked: {e}");
            }
        }
    }

    fn update_queue_metrics(&self) {
        if let Ok(live_len) = self.storage.firehose_live_len() {
            crate::metrics::INGESTER_FIREHOSE_LIVE_LENGTH
                .set(i64::try_from(live_len).unwrap_or(i64::MAX));
        }
        if let Ok(backfill_len) = self.storage.firehose_backfill_len() {
            crate::metrics::INGESTER_FIREHOSE_BACKFILL_LENGTH
                .set(i64::try_from(backfill_len).unwrap_or(i64::MAX));
        }
        if let Ok(label_len) = self.storage.label_live_len() {
            crate::metrics::INGESTER_LABEL_LIVE_LENGTH
                .set(i64::try_from(label_len).unwrap_or(i64::MAX));
        }
    }

    #[cfg(test)]
    fn dequeue_firehose_live_jobs(&self) -> Vec<IndexJobWithMetadata> {
        let mut jobs = Vec::new();
        for _ in 0..*INDEXER_BATCH_SIZE {
            match self.storage.dequeue_firehose_live() {
                Ok(Some((key, job))) => jobs.push((key, job, QueueSource::FirehoseLive)),
                Ok(None) => break,
                Err(e) => {
                    tracing::error!("failed to dequeue firehose_live job: {e}");
                    break;
                }
            }
        }
        jobs
    }

    #[cfg(test)]
    fn dequeue_firehose_backfill_jobs(&self) -> Vec<IndexJobWithMetadata> {
        let mut jobs = Vec::new();
        for _ in 0..*INDEXER_BATCH_SIZE {
            match self.storage.dequeue_firehose_backfill() {
                Ok(Some((key, job))) => jobs.push((key, job, QueueSource::FirehoseBackfill)),
                Ok(None) => break,
                Err(e) => {
                    tracing::error!("failed to dequeue firehose_backfill job: {e}");
                    break;
                }
            }
        }
        jobs
    }

    #[cfg(test)]
    fn dequeue_label_jobs(&self) -> Vec<LabelJobWithMetadata> {
        let mut label_jobs = Vec::new();
        for _ in 0..*INDEXER_BATCH_SIZE {
            match self.storage.dequeue_label_live() {
                Ok(Some((key, label_event))) => {
                    label_jobs.push((key, label_event));
                }
                Ok(None) => break,
                Err(e) => {
                    tracing::error!("failed to dequeue label_live: {e}");
                    break;
                }
            }
        }
        label_jobs
    }

    #[cfg(test)]
    fn dequeue_prioritized_jobs(&self) -> (Vec<IndexJobWithMetadata>, Vec<LabelJobWithMetadata>) {
        let mut jobs = self.dequeue_firehose_live_jobs();
        if jobs.len() < *INDEXER_BATCH_SIZE {
            jobs.extend(self.dequeue_firehose_backfill_jobs());
        }
        let label_jobs = self.dequeue_label_jobs();
        (jobs, label_jobs)
    }

    fn handle_single_job_result(&self, result: JobTaskJoinResult) {
        match result {
            Ok((key, source, Ok(()))) => {
                let remove_result = match source {
                    QueueSource::FirehoseLive => self.storage.remove_firehose_live(&key),
                    QueueSource::FirehoseBackfill => self.storage.remove_firehose_backfill(&key),
                    QueueSource::LabelLive => self.storage.remove_label_live(&key),
                };
                if let Err(e) = remove_result {
                    tracing::error!("failed to remove index job from {:?}: {e}", source);
                }
            }
            Ok((_, _, Err(e))) => {
                crate::metrics::INDEXER_RECORDS_FAILED_TOTAL.inc();
                tracing::error!("index job failed: {e}");
            }
            Err(e) => {
                tracing::error!("task panicked: {e}");
            }
        }
    }

    #[cfg(test)]
    async fn spawn_index_job_tasks(
        &self,
        jobs: Vec<(Vec<u8>, IndexJob, QueueSource)>,
    ) -> Vec<tokio::task::JoinHandle<(Vec<u8>, QueueSource, Result<(), WintermuteError>)>> {
        let mut tasks = Vec::new();
        for (key, job, source) in jobs {
            // Use backfill semaphore for tests (most tests use backfill queue)
            let Ok(permit) = self.semaphore_backfill.clone().acquire_owned().await else {
                break;
            };

            let pool = self.pool_backfill.clone();

            let task = tokio::spawn(async move {
                let result = Self::process_job(&pool, &job).await;
                drop(permit);
                (key, source, result)
            });

            tasks.push(task);
        }
        tasks
    }

    #[cfg(test)]
    async fn handle_job_results(&self, tasks: Vec<tokio::task::JoinHandle<JobTaskResult>>) {
        for task in tasks {
            match task.await {
                Ok((key, source, Ok(()))) => {
                    let remove_result = match source {
                        QueueSource::FirehoseLive => self.storage.remove_firehose_live(&key),
                        QueueSource::FirehoseBackfill => {
                            self.storage.remove_firehose_backfill(&key)
                        }
                        QueueSource::LabelLive => self.storage.remove_label_live(&key),
                    };
                    if let Err(e) = remove_result {
                        tracing::error!("failed to remove index job from {:?}: {e}", source);
                    }
                }
                Ok((_, _, Err(e))) => {
                    crate::metrics::INDEXER_RECORDS_FAILED_TOTAL.inc();
                    tracing::error!("index job failed: {e}");
                }
                Err(e) => {
                    tracing::error!("task panicked: {e}");
                }
            }
        }
    }

    async fn ensure_actor_exists(
        client: &deadpool_postgres::Client,
        did: &str,
        _indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        // Use epoch as indexedAt for new actors so they get picked up for handle resolution immediately
        // The handle resolution loop will update indexedAt after attempting resolution
        let epoch = "1970-01-01T00:00:00Z";
        client
            .execute(
                "INSERT INTO actor (did, \"indexedAt\")
                 VALUES ($1, $2)
                 ON CONFLICT (did) DO NOTHING",
                &[&did, &epoch],
            )
            .await?;
        Ok(())
    }

    /// Resolve and verify a handle for a DID, updating the actor table.
    /// Returns true if handle was successfully resolved and stored.
    pub async fn index_handle(
        client: &deadpool_postgres::Client,
        id_resolver: &Arc<Mutex<IdResolver>>,
        did: &str,
        timestamp: &str,
        force: bool,
    ) -> Result<bool, WintermuteError> {
        // Check if actor exists and needs reindex
        let actor_row = client
            .query_opt(
                "SELECT handle, \"indexedAt\" FROM actor WHERE did = $1",
                &[&did],
            )
            .await?;

        if !force {
            if let Some(row) = &actor_row {
                let current_handle: Option<String> = row.get("handle");
                let indexed_at: String = row.get("indexedAt");

                if let Ok(last_indexed) = chrono::DateTime::parse_from_rfc3339(&indexed_at) {
                    let now = chrono::Utc::now();
                    let age = now.signed_duration_since(last_indexed);

                    let reindex_threshold = if current_handle.is_some() {
                        HANDLE_REINDEX_INTERVAL_VALID
                    } else {
                        HANDLE_REINDEX_INTERVAL_INVALID
                    };

                    if age < chrono::Duration::from_std(reindex_threshold).unwrap_or_default() {
                        return Ok(false);
                    }
                }
            }
        }

        // Resolve DID document to get handle
        let did_doc = {
            let mut resolver = id_resolver.lock().await;
            match resolver.did.resolve(did.to_owned(), Some(true)).await {
                Ok(Some(doc)) => doc,
                Ok(None) => {
                    tracing::debug!("DID not found: {did}");
                    return Ok(false);
                }
                Err(e) => {
                    tracing::debug!("failed to resolve DID {did}: {e}");
                    return Ok(false);
                }
            }
        };

        // Extract handle from alsoKnownAs (format: "at://<handle>")
        let handle = did_doc.also_known_as.as_ref().and_then(|aka| {
            aka.iter()
                .find(|s| s.starts_with("at://"))
                .map(|s| s.trim_start_matches("at://").to_lowercase())
        });

        let handle = match handle {
            Some(h) if !h.is_empty() => h,
            _ => {
                tracing::debug!("no handle found in DID document for {did}");
                // Update actor indexedAt even if no handle found
                client
                    .execute(
                        "UPDATE actor SET \"indexedAt\" = $2 WHERE did = $1",
                        &[&did, &timestamp],
                    )
                    .await?;
                return Ok(false);
            }
        };

        // Verify bidirectional binding: handle -> DID
        let handle_did = {
            let mut resolver = id_resolver.lock().await;
            match resolver.handle.resolve(&handle).await {
                Ok(Some(resolved_did)) => resolved_did,
                Ok(None) => {
                    tracing::debug!("handle {handle} does not resolve to a DID");
                    client
                        .execute(
                            "UPDATE actor SET handle = NULL, \"indexedAt\" = $2 WHERE did = $1",
                            &[&did, &timestamp],
                        )
                        .await?;
                    return Ok(false);
                }
                Err(e) => {
                    tracing::debug!("failed to resolve handle {handle}: {e}");
                    return Ok(false);
                }
            }
        };

        // Handle is only valid if it resolves back to this DID
        let verified_handle = if handle_did == did {
            Some(handle)
        } else {
            tracing::debug!("handle {handle} resolves to {handle_did}, expected {did}");
            None
        };

        // Handle contention: if another actor has this handle, remove it
        if let Some(ref h) = verified_handle {
            client
                .execute(
                    "UPDATE actor SET handle = NULL WHERE handle = $1 AND did != $2",
                    &[&h, &did],
                )
                .await?;
        }

        // Update actor with handle
        client
            .execute(
                "INSERT INTO actor (did, handle, \"indexedAt\")
                 VALUES ($1, $2, $3)
                 ON CONFLICT (did) DO UPDATE SET
                   handle = EXCLUDED.handle,
                   \"indexedAt\" = EXCLUDED.\"indexedAt\"",
                &[&did, &verified_handle, &timestamp],
            )
            .await?;

        Ok(verified_handle.is_some())
    }

    async fn insert_generic_record(
        client: &deadpool_postgres::Client,
        uri: &str,
        cid: &str,
        did: &str,
        json: &serde_json::Value,
        rev: &str,
        indexed_at: &str,
    ) -> Result<bool, WintermuteError> {
        let json_str = serde_json::to_string(json)
            .map_err(|e| WintermuteError::Serialization(format!("json stringify failed: {e}")))?;

        tracing::debug!("inserting generic record: uri={uri}, rev={rev}, did={did}");

        let result = client
            .query_opt(
                "INSERT INTO record (uri, cid, did, json, rev, \"indexedAt\")
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (uri) DO UPDATE SET
                   rev = EXCLUDED.rev,
                   cid = EXCLUDED.cid,
                   json = EXCLUDED.json,
                   \"indexedAt\" = EXCLUDED.\"indexedAt\"
                 WHERE record.rev <= EXCLUDED.rev
                 RETURNING uri",
                &[&uri, &cid, &did, &json_str, &rev, &indexed_at],
            )
            .await?;

        let applied = result.is_some();
        tracing::debug!(
            "generic record insert result for {uri}: applied={applied}, result={:?}",
            result
        );
        if !applied {
            tracing::trace!("stale write rejected for {uri} with rev {rev}");
        }
        Ok(applied)
    }

    async fn delete_generic_record(
        client: &deadpool_postgres::Client,
        uri: &str,
        rev: &str,
    ) -> Result<bool, WintermuteError> {
        // Delete the record from the record table (matching TypeScript dataplane behavior)
        // Only delete if the record's rev is <= the delete operation's rev
        let result = client
            .query_opt(
                "DELETE FROM record
                 WHERE uri = $1 AND rev <= $2
                 RETURNING uri",
                &[&uri, &rev],
            )
            .await?;

        // Also delete from duplicate_record table
        client
            .execute("DELETE FROM duplicate_record WHERE uri = $1", &[&uri])
            .await?;

        Ok(result.is_some())
    }

    pub async fn process_job(pool: &Pool, job: &IndexJob) -> Result<(), WintermuteError> {
        use crate::metrics;

        tracing::debug!(
            "process_job called: uri={}, action={:?}, has_record={}",
            job.uri,
            job.action,
            job.record.is_some()
        );

        metrics::INDEXER_RECORDS_PROCESSED_TOTAL.inc();

        let uri = AtUri::new(job.uri.clone(), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;

        let did = uri.get_hostname();
        let collection = uri.get_collection();
        let rkey = uri.get_rkey();

        tracing::debug!("parsed uri: did={did}, collection={collection}, rkey={rkey}");

        let client = pool.get().await?;
        tracing::debug!("got database client");

        match job.action {
            WriteAction::Create | WriteAction::Update => {
                tracing::debug!("processing create/update action");

                // Ensure actor row exists for this DID (cached to avoid redundant DB calls)
                if !ACTOR_CACHE.contains_key(did.as_str()) {
                    Self::ensure_actor_exists(&client, did.as_str(), &job.indexed_at).await?;
                    if ACTOR_CACHE.len() < ACTOR_CACHE_MAX_SIZE {
                        ACTOR_CACHE.insert(did.clone(), ());
                    }
                }

                let record_json = job.record.as_ref().ok_or_else(|| {
                    WintermuteError::Other("missing record for create/update".into())
                })?;

                let applied = Self::insert_generic_record(
                    &client,
                    &job.uri,
                    &job.cid,
                    did.as_str(),
                    record_json,
                    &job.rev,
                    &job.indexed_at,
                )
                .await?;

                if !applied {
                    metrics::INDEXER_STALE_WRITES_SKIPPED_TOTAL.inc();
                    tracing::debug!("skipping stale write for {}", job.uri);
                    return Ok(());
                }
                tracing::debug!(
                    "proceeding to collection-specific indexing for {}",
                    collection
                );

                match collection.as_str() {
                    "app.bsky.feed.post" => {
                        metrics::INDEXER_POST_EVENTS_TOTAL.inc();
                        Self::index_post(
                            &client,
                            did.as_str(),
                            rkey.as_str(),
                            record_json,
                            &job.cid,
                            &job.indexed_at,
                        )
                        .await?;
                    }
                    "app.bsky.feed.like" => {
                        metrics::INDEXER_LIKE_EVENTS_TOTAL.inc();
                        Self::index_like(
                            &client,
                            did.as_str(),
                            rkey.as_str(),
                            record_json,
                            &job.cid,
                            &job.indexed_at,
                        )
                        .await?;
                    }
                    "app.bsky.graph.follow" => {
                        metrics::INDEXER_FOLLOW_EVENTS_TOTAL.inc();
                        Self::index_follow(
                            &client,
                            did.as_str(),
                            rkey.as_str(),
                            record_json,
                            &job.cid,
                            &job.indexed_at,
                        )
                        .await?;
                    }
                    "app.bsky.feed.repost" => {
                        metrics::INDEXER_REPOST_EVENTS_TOTAL.inc();
                        Self::index_repost(
                            &client,
                            did.as_str(),
                            rkey.as_str(),
                            record_json,
                            &job.cid,
                            &job.indexed_at,
                        )
                        .await?;
                    }
                    "app.bsky.graph.block" => {
                        metrics::INDEXER_BLOCK_EVENTS_TOTAL.inc();
                        Self::index_block(
                            &client,
                            did.as_str(),
                            rkey.as_str(),
                            record_json,
                            &job.cid,
                            &job.indexed_at,
                        )
                        .await?;
                    }
                    "app.bsky.actor.profile" => {
                        metrics::INDEXER_PROFILE_EVENTS_TOTAL.inc();
                        Self::index_profile(
                            &client,
                            did.as_str(),
                            rkey.as_str(),
                            record_json,
                            &job.cid,
                            &job.indexed_at,
                        )
                        .await?;
                    }
                    "app.bsky.feed.generator" => {
                        Self::index_feed_generator(
                            &client,
                            did.as_str(),
                            rkey.as_str(),
                            record_json,
                            &job.cid,
                            &job.indexed_at,
                        )
                        .await?;
                    }
                    "app.bsky.graph.list" => {
                        Self::index_list(
                            &client,
                            did.as_str(),
                            rkey.as_str(),
                            record_json,
                            &job.cid,
                            &job.indexed_at,
                        )
                        .await?;
                    }
                    "app.bsky.graph.listitem" => {
                        Self::index_list_item(
                            &client,
                            did.as_str(),
                            rkey.as_str(),
                            record_json,
                            &job.cid,
                            &job.indexed_at,
                        )
                        .await?;
                    }
                    "app.bsky.graph.listblock" => {
                        Self::index_list_block(
                            &client,
                            did.as_str(),
                            rkey.as_str(),
                            record_json,
                            &job.cid,
                            &job.indexed_at,
                        )
                        .await?;
                    }
                    "app.bsky.graph.starterpack" => {
                        Self::index_starter_pack(
                            &client,
                            did.as_str(),
                            rkey.as_str(),
                            record_json,
                            &job.cid,
                            &job.indexed_at,
                        )
                        .await?;
                    }
                    "app.bsky.labeler.service" => {
                        Self::index_labeler(
                            &client,
                            did.as_str(),
                            rkey.as_str(),
                            record_json,
                            &job.cid,
                            &job.indexed_at,
                        )
                        .await?;
                    }
                    "app.bsky.feed.threadgate" => {
                        Self::index_threadgate(
                            &client,
                            did.as_str(),
                            rkey.as_str(),
                            record_json,
                            &job.cid,
                            &job.indexed_at,
                        )
                        .await?;
                    }
                    "app.bsky.feed.postgate" => {
                        Self::index_postgate(
                            &client,
                            did.as_str(),
                            rkey.as_str(),
                            record_json,
                            &job.cid,
                            &job.indexed_at,
                        )
                        .await?;
                    }
                    "chat.bsky.actor.declaration" => {
                        Self::index_chat_declaration(
                            &client,
                            did.as_str(),
                            rkey.as_str(),
                            record_json,
                            &job.cid,
                            &job.indexed_at,
                        )
                        .await?;
                    }
                    "app.bsky.notification.declaration" => {
                        Self::index_notif_declaration(
                            &client,
                            did.as_str(),
                            rkey.as_str(),
                            record_json,
                            &job.cid,
                            &job.indexed_at,
                        )
                        .await?;
                    }
                    "app.bsky.actor.status" => {
                        Self::index_status(
                            &client,
                            did.as_str(),
                            rkey.as_str(),
                            record_json,
                            &job.cid,
                            &job.indexed_at,
                        )
                        .await?;
                    }
                    "app.bsky.verification.proof" => {
                        Self::index_verification(
                            &client,
                            did.as_str(),
                            rkey.as_str(),
                            record_json,
                            &job.cid,
                            &job.indexed_at,
                        )
                        .await?;
                    }
                    _ => {}
                }
            }
            WriteAction::Delete => {
                let applied = Self::delete_generic_record(&client, &job.uri, &job.rev).await?;

                if !applied {
                    return Ok(());
                }

                match collection.as_str() {
                    "app.bsky.feed.post" => {
                        Self::delete_post(&client, did.as_str(), rkey.as_str()).await?;
                    }
                    "app.bsky.feed.like" => {
                        Self::delete_like(&client, did.as_str(), rkey.as_str()).await?;
                    }
                    "app.bsky.graph.follow" => {
                        Self::delete_follow(&client, did.as_str(), rkey.as_str()).await?;
                    }
                    "app.bsky.feed.repost" => {
                        Self::delete_repost(&client, did.as_str(), rkey.as_str()).await?;
                    }
                    "app.bsky.graph.block" => {
                        Self::delete_block(&client, did.as_str(), rkey.as_str()).await?;
                    }
                    "app.bsky.actor.profile" => {
                        Self::delete_profile(&client, did.as_str(), rkey.as_str()).await?;
                    }
                    "app.bsky.feed.generator" => {
                        Self::delete_feed_generator(&client, did.as_str(), rkey.as_str()).await?;
                    }
                    "app.bsky.graph.list" => {
                        Self::delete_list(&client, did.as_str(), rkey.as_str()).await?;
                    }
                    "app.bsky.graph.listitem" => {
                        Self::delete_list_item(&client, did.as_str(), rkey.as_str()).await?;
                    }
                    "app.bsky.graph.listblock" => {
                        Self::delete_list_block(&client, did.as_str(), rkey.as_str()).await?;
                    }
                    "app.bsky.graph.starterpack" => {
                        Self::delete_starter_pack(&client, did.as_str(), rkey.as_str()).await?;
                    }
                    "app.bsky.labeler.service" => {
                        Self::delete_labeler(&client, did.as_str(), rkey.as_str()).await?;
                    }
                    "app.bsky.feed.threadgate" => {
                        Self::delete_threadgate(&client, did.as_str(), rkey.as_str()).await?;
                    }
                    "app.bsky.feed.postgate" => {
                        Self::delete_postgate(&client, did.as_str(), rkey.as_str()).await?;
                    }
                    "chat.bsky.actor.declaration" => {
                        Self::delete_chat_declaration(&client, did.as_str(), rkey.as_str()).await?;
                    }
                    "app.bsky.notification.declaration" => {
                        Self::delete_notif_declaration(&client, did.as_str(), rkey.as_str())
                            .await?;
                    }
                    "app.bsky.actor.status" => {
                        Self::delete_status(&client, did.as_str(), rkey.as_str()).await?;
                    }
                    "app.bsky.verification.proof" => {
                        Self::delete_verification(&client, did.as_str(), rkey.as_str()).await?;
                    }
                    _ => {}
                }
            }
        }

        Ok(())
    }

    /// Process a batch of jobs efficiently using batch INSERT statements.
    /// This dramatically improves throughput by reducing database round-trips.
    /// Uses parallel COPY operations for different collection types.
    pub async fn process_jobs_batch(
        pool: &Pool,
        jobs: &[(Vec<u8>, IndexJob)],
    ) -> Vec<(Vec<u8>, Result<(), WintermuteError>)> {
        use crate::metrics;

        if jobs.is_empty() {
            return Vec::new();
        }

        let mut results: Vec<(Vec<u8>, Result<(), WintermuteError>)> =
            Vec::with_capacity(jobs.len());

        // Separate creates/updates from deletes
        let mut creates: Vec<&(Vec<u8>, IndexJob)> = Vec::new();
        let mut deletes: Vec<&(Vec<u8>, IndexJob)> = Vec::new();

        for job_tuple in jobs {
            match job_tuple.1.action {
                WriteAction::Create | WriteAction::Update => creates.push(job_tuple),
                WriteAction::Delete => deletes.push(job_tuple),
            }
        }

        // Process creates in batch (uses parallel COPY for different collection types)
        if !creates.is_empty() {
            let batch_results = Self::batch_insert_records(pool, &creates).await;
            results.extend(batch_results);
        }

        // Process deletes individually (they're less common and more complex)
        for (key, job) in deletes {
            let result = Self::process_job(pool, job).await;
            metrics::INDEXER_RECORDS_PROCESSED_TOTAL.inc();
            results.push((key.clone(), result));
        }

        results
    }

    /// Batch insert records using `PostgreSQL` `COPY` protocol for high throughput.
    /// Uses parallel COPY operations for different collection types to maximize throughput.
    async fn batch_insert_records(
        pool: &Pool,
        jobs: &[&(Vec<u8>, IndexJob)],
    ) -> Vec<(Vec<u8>, Result<(), WintermuteError>)> {
        use crate::metrics;
        use std::time::Instant;

        let batch_start = Instant::now();
        let mut results: Vec<(Vec<u8>, Result<(), WintermuteError>)> =
            Vec::with_capacity(jobs.len());

        // Get a client for the initial actor/record operations
        let client = match pool.get().await {
            Ok(c) => c,
            Err(e) => {
                let err_msg = format!("pool error: {e}");
                for (key, _) in jobs {
                    results.push(((*key).clone(), Err(WintermuteError::Other(err_msg.clone()))));
                }
                return results;
            }
        };

        // Parse all URIs and collect data
        let parse_start = Instant::now();
        let mut parsed_jobs: Vec<ParsedJob<'_>> = Vec::with_capacity(jobs.len());

        for (key, job) in jobs {
            match AtUri::new(job.uri.clone(), None) {
                Ok(uri) => {
                    let did = uri.get_hostname().clone();
                    let collection = uri.get_collection().clone();
                    let rkey = uri.get_rkey().clone();
                    parsed_jobs.push(ParsedJob {
                        key,
                        job,
                        uri,
                        did,
                        collection,
                        rkey,
                    });
                }
                Err(e) => {
                    results.push((
                        (*key).clone(),
                        Err(WintermuteError::Other(format!("invalid uri: {e}"))),
                    ));
                }
            }
        }
        let parse_ms = parse_start.elapsed().as_millis();

        if parsed_jobs.is_empty() {
            return results;
        }

        // Batch 1: Ensure all actors exist using COPY
        let actors_start = Instant::now();
        let unique_dids: Vec<&str> = parsed_jobs
            .iter()
            .map(|p| p.did.as_str())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        if let Err(e) = bulk::copy_ensure_actors(&client, &unique_dids).await {
            tracing::error!("COPY actor insert failed: {e}");
            // Continue anyway, individual records may still work
        }
        let actors_ms = actors_start.elapsed().as_millis();

        // Batch 2: Insert all records into the record table using COPY
        let records_start = Instant::now();
        let record_data: Vec<_> = parsed_jobs
            .iter()
            .map(|pj| {
                (
                    pj.job.uri.clone(),
                    pj.job.cid.clone(),
                    pj.did.clone(),
                    pj.job
                        .record
                        .as_ref()
                        .map(|r| serde_json::to_string(r).unwrap_or_default())
                        .unwrap_or_default(),
                    pj.job.rev.clone(),
                    pj.job.indexed_at.clone(),
                )
            })
            .collect();

        let record_results = match bulk::copy_insert_records(&client, &record_data).await {
            Ok(results) => results,
            Err(e) => {
                tracing::error!("COPY record insert failed: {e}");
                vec![false; parsed_jobs.len()]
            }
        };
        let records_ms = records_start.elapsed().as_millis();

        // Track which jobs were applied (not stale)
        let mut applied_jobs: Vec<&ParsedJob<'_>> = Vec::new();
        for (i, applied) in record_results.iter().enumerate() {
            if *applied {
                applied_jobs.push(&parsed_jobs[i]);
            } else {
                metrics::INDEXER_STALE_WRITES_SKIPPED_TOTAL.inc();
            }
        }

        // Batch 3: Insert collection-specific records using PARALLEL COPY
        // Pre-separate jobs by collection type for parallel processing
        let collections_start = Instant::now();
        let mut posts: Vec<&ParsedJob<'_>> = Vec::new();
        let mut likes: Vec<&ParsedJob<'_>> = Vec::new();
        let mut follows: Vec<&ParsedJob<'_>> = Vec::new();
        let mut reposts: Vec<&ParsedJob<'_>> = Vec::new();
        let mut blocks: Vec<&ParsedJob<'_>> = Vec::new();
        let mut profiles: Vec<&ParsedJob<'_>> = Vec::new();
        let mut others: Vec<&ParsedJob<'_>> = Vec::new();

        for job in &applied_jobs {
            match job.collection.as_str() {
                "app.bsky.feed.post" => posts.push(job),
                "app.bsky.feed.like" => likes.push(job),
                "app.bsky.graph.follow" => follows.push(job),
                "app.bsky.feed.repost" => reposts.push(job),
                "app.bsky.graph.block" => blocks.push(job),
                "app.bsky.actor.profile" => profiles.push(job),
                _ => others.push(job),
            }
        }

        // Run COPY operations in PARALLEL using separate connections
        // Each collection type writes to different tables, so no lock contention
        let (
            posts_result,
            likes_result,
            follows_result,
            reposts_result,
            blocks_result,
            profiles_result,
        ) = tokio::join!(
            Self::parallel_copy_posts(pool, &posts),
            Self::parallel_copy_likes(pool, &likes),
            Self::parallel_copy_follows(pool, &follows),
            Self::parallel_copy_reposts(pool, &reposts),
            Self::parallel_copy_blocks(pool, &blocks),
            Self::parallel_copy_profiles(pool, &profiles),
        );

        // Collect timing results (only include non-empty collections)
        let mut collection_timings: Vec<(String, u128, usize)> = Vec::new();

        let (ms, count, err) = posts_result;
        if count > 0 {
            collection_timings.push(("post".to_owned(), ms, count));
        }
        if let Some(e) = err {
            tracing::error!("COPY batch insert for posts failed: {e}");
        }

        let (ms, count, err) = likes_result;
        if count > 0 {
            collection_timings.push(("like".to_owned(), ms, count));
        }
        if let Some(e) = err {
            tracing::error!("COPY batch insert for likes failed: {e}");
        }

        let (ms, count, err) = follows_result;
        if count > 0 {
            collection_timings.push(("follow".to_owned(), ms, count));
        }
        if let Some(e) = err {
            tracing::error!("COPY batch insert for follows failed: {e}");
        }

        let (ms, count, err) = reposts_result;
        if count > 0 {
            collection_timings.push(("repost".to_owned(), ms, count));
        }
        if let Some(e) = err {
            tracing::error!("COPY batch insert for reposts failed: {e}");
        }

        let (ms, count, err) = blocks_result;
        if count > 0 {
            collection_timings.push(("block".to_owned(), ms, count));
        }
        if let Some(e) = err {
            tracing::error!("COPY batch insert for blocks failed: {e}");
        }

        let (ms, count, err) = profiles_result;
        if count > 0 {
            collection_timings.push(("profile".to_owned(), ms, count));
        }
        if let Some(e) = err {
            tracing::error!("COPY batch insert for profiles failed: {e}");
        }

        // Process "other" collection types sequentially (less common)
        if !others.is_empty() {
            let other_start = Instant::now();
            for pj in &others {
                if let Some(record) = &pj.job.record {
                    if let Err(e) = Self::process_collection_specific(
                        &client,
                        &pj.collection,
                        &pj.did,
                        &pj.rkey,
                        record,
                        &pj.job.cid,
                        &pj.job.indexed_at,
                    )
                    .await
                    {
                        tracing::warn!(
                            "process_collection_specific failed for {}: {e}",
                            pj.job.uri
                        );
                    }
                }
            }
            collection_timings.push((
                "other".to_owned(),
                other_start.elapsed().as_millis(),
                others.len(),
            ));
        }
        let collections_ms = collections_start.elapsed().as_millis();

        // All creates succeeded (or were stale)
        for pj in &parsed_jobs {
            metrics::INDEXER_RECORDS_PROCESSED_TOTAL.inc();
            results.push(((*pj.key).clone(), Ok(())));
        }

        let total_ms = batch_start.elapsed().as_millis();
        let applied_count = applied_jobs.len();

        // Log timing breakdown
        let col_breakdown: String = collection_timings
            .iter()
            .map(|(c, ms, n)| format!("{}={}ms({})", c.rsplit('.').next().unwrap_or(c), ms, n))
            .collect::<Vec<_>>()
            .join(", ");

        tracing::info!(
            "batch timing: total={}ms, parse={}ms, actors={}ms, records={}ms, collections={}ms [{}] | jobs={}, applied={}",
            total_ms,
            parse_ms,
            actors_ms,
            records_ms,
            collections_ms,
            col_breakdown,
            jobs.len(),
            applied_count
        );

        results
    }

    // Legacy batch functions - kept as fallbacks (now using COPY protocol)
    #[allow(dead_code)]
    async fn batch_ensure_actors(
        client: &deadpool_postgres::Client,
        dids: &[&str],
    ) -> Result<(), WintermuteError> {
        if dids.is_empty() {
            return Ok(());
        }

        let epoch = "1970-01-01T00:00:00Z";
        let dids_vec: Vec<String> = dids.iter().map(|s| (*s).to_owned()).collect();

        client
            .execute(
                "INSERT INTO actor (did, \"indexedAt\")
                 SELECT unnest($1::text[]), $2
                 ON CONFLICT (did) DO NOTHING",
                &[&dids_vec, &epoch],
            )
            .await?;

        Ok(())
    }

    #[allow(dead_code)]
    async fn batch_insert_record_table(
        client: &deadpool_postgres::Client,
        jobs: &[ParsedJob<'_>],
    ) -> Vec<bool> {
        if jobs.is_empty() {
            return Vec::new();
        }

        // Build arrays for unnest
        let mut uris: Vec<String> = Vec::with_capacity(jobs.len());
        let mut cids: Vec<String> = Vec::with_capacity(jobs.len());
        let mut dids: Vec<String> = Vec::with_capacity(jobs.len());
        let mut jsons: Vec<String> = Vec::with_capacity(jobs.len());
        let mut revs: Vec<String> = Vec::with_capacity(jobs.len());
        let mut indexed_ats: Vec<String> = Vec::with_capacity(jobs.len());

        for pj in jobs {
            uris.push(pj.job.uri.clone());
            cids.push(pj.job.cid.clone());
            dids.push(pj.did.clone());
            jsons.push(
                pj.job
                    .record
                    .as_ref()
                    .map(|r| serde_json::to_string(r).unwrap_or_default())
                    .unwrap_or_default(),
            );
            revs.push(pj.job.rev.clone());
            indexed_ats.push(pj.job.indexed_at.clone());
        }

        // Use INSERT with ON CONFLICT and track which rows were applied
        // We return the uri of rows that were actually inserted/updated
        let result = client
            .query(
                "INSERT INTO record (uri, cid, did, json, rev, \"indexedAt\")
                 SELECT * FROM unnest($1::text[], $2::text[], $3::text[], $4::text[], $5::text[], $6::text[])
                 ON CONFLICT (uri) DO UPDATE SET
                   rev = EXCLUDED.rev,
                   cid = EXCLUDED.cid,
                   json = EXCLUDED.json,
                   \"indexedAt\" = EXCLUDED.\"indexedAt\"
                 WHERE record.rev <= EXCLUDED.rev
                 RETURNING uri",
                &[&uris, &cids, &dids, &jsons, &revs, &indexed_ats],
            )
            .await;

        match result {
            Ok(rows) => {
                let applied_uris: std::collections::HashSet<String> =
                    rows.iter().map(|r| r.get::<_, String>(0)).collect();
                jobs.iter()
                    .map(|pj| applied_uris.contains(&pj.job.uri))
                    .collect()
            }
            Err(e) => {
                tracing::error!("batch record insert failed: {e}");
                // Assume all failed
                vec![false; jobs.len()]
            }
        }
    }

    async fn process_collection_specific(
        client: &deadpool_postgres::Client,
        collection: &str,
        did: &str,
        rkey: &str,
        record: &serde_json::Value,
        cid: &str,
        indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        match collection {
            "app.bsky.feed.generator" => {
                Self::index_feed_generator(client, did, rkey, record, cid, indexed_at).await
            }
            "app.bsky.graph.list" => {
                Self::index_list(client, did, rkey, record, cid, indexed_at).await
            }
            "app.bsky.graph.listitem" => {
                Self::index_list_item(client, did, rkey, record, cid, indexed_at).await
            }
            "app.bsky.graph.listblock" => {
                Self::index_list_block(client, did, rkey, record, cid, indexed_at).await
            }
            "app.bsky.graph.starterpack" => {
                Self::index_starter_pack(client, did, rkey, record, cid, indexed_at).await
            }
            "app.bsky.labeler.service" => {
                Self::index_labeler(client, did, rkey, record, cid, indexed_at).await
            }
            "app.bsky.feed.threadgate" => {
                Self::index_threadgate(client, did, rkey, record, cid, indexed_at).await
            }
            "app.bsky.feed.postgate" => {
                Self::index_postgate(client, did, rkey, record, cid, indexed_at).await
            }
            "chat.bsky.actor.declaration" => {
                Self::index_chat_declaration(client, did, rkey, record, cid, indexed_at).await
            }
            "app.bsky.notification.declaration" => {
                Self::index_notif_declaration(client, did, rkey, record, cid, indexed_at).await
            }
            "app.bsky.actor.status" => {
                Self::index_status(client, did, rkey, record, cid, indexed_at).await
            }
            "app.bsky.verification.proof" => {
                Self::index_verification(client, did, rkey, record, cid, indexed_at).await
            }
            _ => Ok(()),
        }
    }

    #[allow(dead_code)]
    async fn batch_insert_posts(
        client: &deadpool_postgres::Client,
        jobs: &[&ParsedJob<'_>],
    ) -> Result<(), WintermuteError> {
        use crate::metrics;

        if jobs.is_empty() {
            return Ok(());
        }

        let mut uris: Vec<String> = Vec::with_capacity(jobs.len());
        let mut cids: Vec<String> = Vec::with_capacity(jobs.len());
        let mut creators: Vec<String> = Vec::with_capacity(jobs.len());
        let mut texts: Vec<String> = Vec::with_capacity(jobs.len());
        let mut created_ats: Vec<String> = Vec::with_capacity(jobs.len());
        let mut indexed_ats: Vec<String> = Vec::with_capacity(jobs.len());

        // For feed_item table
        let mut item_post_uris: Vec<String> = Vec::with_capacity(jobs.len());
        let mut item_cids: Vec<String> = Vec::with_capacity(jobs.len());
        let mut item_originator_dids: Vec<String> = Vec::with_capacity(jobs.len());
        let mut sort_ats: Vec<String> = Vec::with_capacity(jobs.len());

        for pj in jobs {
            if let Some(record) = &pj.job.record {
                let uri = pj.uri.to_string();
                let text = sanitize_text(record.get("text").and_then(|v| v.as_str()).unwrap_or(""));
                let created_at = record
                    .get("createdAt")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&pj.job.indexed_at)
                    .to_owned();
                let sort_at = if pj.job.indexed_at < created_at {
                    pj.job.indexed_at.clone()
                } else {
                    created_at.clone()
                };

                uris.push(uri.clone());
                cids.push(pj.job.cid.clone());
                creators.push(pj.did.clone());
                texts.push(text);
                created_ats.push(created_at);
                indexed_ats.push(pj.job.indexed_at.clone());

                item_post_uris.push(uri);
                item_cids.push(pj.job.cid.clone());
                item_originator_dids.push(pj.did.clone());
                sort_ats.push(sort_at);

                metrics::INDEXER_POST_EVENTS_TOTAL.inc();
            }
        }

        // Batch insert posts
        client
            .execute(
                "INSERT INTO post (uri, cid, creator, text, \"createdAt\", \"indexedAt\")
                 SELECT * FROM unnest($1::text[], $2::text[], $3::text[], $4::text[], $5::text[], $6::text[])
                 ON CONFLICT DO NOTHING",
                &[&uris, &cids, &creators, &texts, &created_ats, &indexed_ats],
            )
            .await?;

        // Batch insert feed_items
        client
            .execute(
                "INSERT INTO feed_item (type, uri, cid, \"postUri\", \"originatorDid\", \"sortAt\")
                 SELECT 'post', uri, cid, uri, did, sort_at
                 FROM unnest($1::text[], $2::text[], $3::text[], $4::text[]) AS t(uri, cid, did, sort_at)
                 ON CONFLICT DO NOTHING",
                &[&item_post_uris, &item_cids, &item_originator_dids, &sort_ats],
            )
            .await?;

        Ok(())
    }

    #[allow(dead_code)]
    async fn batch_insert_likes(
        client: &deadpool_postgres::Client,
        jobs: &[&ParsedJob<'_>],
    ) -> Result<(), WintermuteError> {
        use crate::metrics;

        if jobs.is_empty() {
            return Ok(());
        }

        let mut uris: Vec<String> = Vec::with_capacity(jobs.len());
        let mut cids: Vec<String> = Vec::with_capacity(jobs.len());
        let mut creators: Vec<String> = Vec::with_capacity(jobs.len());
        let mut subjects: Vec<String> = Vec::with_capacity(jobs.len());
        let mut subject_cids: Vec<String> = Vec::with_capacity(jobs.len());
        let mut created_ats: Vec<String> = Vec::with_capacity(jobs.len());
        let mut indexed_ats: Vec<String> = Vec::with_capacity(jobs.len());

        for pj in jobs {
            if let Some(record) = &pj.job.record {
                let uri = pj.uri.to_string();
                let subject_obj = record.get("subject");
                let subject_uri = subject_obj
                    .and_then(|s| s.get("uri"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let subject_cid = subject_obj
                    .and_then(|s| s.get("cid"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let created_at = record
                    .get("createdAt")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&pj.job.indexed_at);

                uris.push(uri);
                cids.push(pj.job.cid.clone());
                creators.push(pj.did.clone());
                subjects.push(subject_uri.to_owned());
                subject_cids.push(subject_cid.to_owned());
                created_ats.push(created_at.to_owned());
                indexed_ats.push(pj.job.indexed_at.clone());

                metrics::INDEXER_LIKE_EVENTS_TOTAL.inc();
            }
        }

        client
            .execute(
                "INSERT INTO \"like\" (uri, cid, creator, subject, \"subjectCid\", \"createdAt\", \"indexedAt\")
                 SELECT * FROM unnest($1::text[], $2::text[], $3::text[], $4::text[], $5::text[], $6::text[], $7::text[])
                 ON CONFLICT DO NOTHING",
                &[&uris, &cids, &creators, &subjects, &subject_cids, &created_ats, &indexed_ats],
            )
            .await?;

        Ok(())
    }

    #[allow(dead_code)]
    async fn batch_insert_follows(
        client: &deadpool_postgres::Client,
        jobs: &[&ParsedJob<'_>],
    ) -> Result<(), WintermuteError> {
        use crate::metrics;

        if jobs.is_empty() {
            return Ok(());
        }

        let mut uris: Vec<String> = Vec::with_capacity(jobs.len());
        let mut cids: Vec<String> = Vec::with_capacity(jobs.len());
        let mut creators: Vec<String> = Vec::with_capacity(jobs.len());
        let mut subjects: Vec<String> = Vec::with_capacity(jobs.len());
        let mut created_ats: Vec<String> = Vec::with_capacity(jobs.len());
        let mut indexed_ats: Vec<String> = Vec::with_capacity(jobs.len());

        for pj in jobs {
            if let Some(record) = &pj.job.record {
                let uri = pj.uri.to_string();
                let subject = record.get("subject").and_then(|v| v.as_str()).unwrap_or("");
                let created_at = record
                    .get("createdAt")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&pj.job.indexed_at);

                uris.push(uri);
                cids.push(pj.job.cid.clone());
                creators.push(pj.did.clone());
                subjects.push(subject.to_owned());
                created_ats.push(created_at.to_owned());
                indexed_ats.push(pj.job.indexed_at.clone());

                metrics::INDEXER_FOLLOW_EVENTS_TOTAL.inc();
            }
        }

        client
            .execute(
                "INSERT INTO follow (uri, cid, creator, \"subjectDid\", \"createdAt\", \"indexedAt\")
                 SELECT * FROM unnest($1::text[], $2::text[], $3::text[], $4::text[], $5::text[], $6::text[])
                 ON CONFLICT DO NOTHING",
                &[&uris, &cids, &creators, &subjects, &created_ats, &indexed_ats],
            )
            .await?;

        Ok(())
    }

    #[allow(dead_code)]
    async fn batch_insert_reposts(
        client: &deadpool_postgres::Client,
        jobs: &[&ParsedJob<'_>],
    ) -> Result<(), WintermuteError> {
        use crate::metrics;

        if jobs.is_empty() {
            return Ok(());
        }

        let mut uris: Vec<String> = Vec::with_capacity(jobs.len());
        let mut cids: Vec<String> = Vec::with_capacity(jobs.len());
        let mut creators: Vec<String> = Vec::with_capacity(jobs.len());
        let mut subjects: Vec<String> = Vec::with_capacity(jobs.len());
        let mut subject_cids: Vec<String> = Vec::with_capacity(jobs.len());
        let mut created_ats: Vec<String> = Vec::with_capacity(jobs.len());
        let mut indexed_ats: Vec<String> = Vec::with_capacity(jobs.len());

        // For feed_item table
        let mut item_uris: Vec<String> = Vec::with_capacity(jobs.len());
        let mut item_cids: Vec<String> = Vec::with_capacity(jobs.len());
        let mut item_post_uris: Vec<String> = Vec::with_capacity(jobs.len());
        let mut item_originator_dids: Vec<String> = Vec::with_capacity(jobs.len());
        let mut sort_ats: Vec<String> = Vec::with_capacity(jobs.len());

        for pj in jobs {
            if let Some(record) = &pj.job.record {
                let uri = pj.uri.to_string();
                let subject_obj = record.get("subject");
                let subject_uri = subject_obj
                    .and_then(|s| s.get("uri"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let subject_cid = subject_obj
                    .and_then(|s| s.get("cid"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let created_at = record
                    .get("createdAt")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&pj.job.indexed_at)
                    .to_owned();
                let sort_at = if pj.job.indexed_at < created_at {
                    pj.job.indexed_at.clone()
                } else {
                    created_at.clone()
                };

                uris.push(uri.clone());
                cids.push(pj.job.cid.clone());
                creators.push(pj.did.clone());
                subjects.push(subject_uri.to_owned());
                subject_cids.push(subject_cid.to_owned());
                created_ats.push(created_at.clone());
                indexed_ats.push(pj.job.indexed_at.clone());

                item_uris.push(uri);
                item_cids.push(pj.job.cid.clone());
                item_post_uris.push(subject_uri.to_owned());
                item_originator_dids.push(pj.did.clone());
                sort_ats.push(sort_at);

                metrics::INDEXER_REPOST_EVENTS_TOTAL.inc();
            }
        }

        // Batch insert reposts
        client
            .execute(
                "INSERT INTO repost (uri, cid, creator, subject, \"subjectCid\", \"createdAt\", \"indexedAt\")
                 SELECT * FROM unnest($1::text[], $2::text[], $3::text[], $4::text[], $5::text[], $6::text[], $7::text[])
                 ON CONFLICT DO NOTHING",
                &[&uris, &cids, &creators, &subjects, &subject_cids, &created_ats, &indexed_ats],
            )
            .await?;

        // Batch insert feed_items
        client
            .execute(
                "INSERT INTO feed_item (type, uri, cid, \"postUri\", \"originatorDid\", \"sortAt\")
                 SELECT 'repost', uri, cid, post_uri, did, sort_at
                 FROM unnest($1::text[], $2::text[], $3::text[], $4::text[], $5::text[]) AS t(uri, cid, post_uri, did, sort_at)
                 ON CONFLICT DO NOTHING",
                &[&item_uris, &item_cids, &item_post_uris, &item_originator_dids, &sort_ats],
            )
            .await?;

        Ok(())
    }

    #[allow(dead_code)]
    async fn batch_insert_blocks(
        client: &deadpool_postgres::Client,
        jobs: &[&ParsedJob<'_>],
    ) -> Result<(), WintermuteError> {
        use crate::metrics;

        if jobs.is_empty() {
            return Ok(());
        }

        let mut uris: Vec<String> = Vec::with_capacity(jobs.len());
        let mut cids: Vec<String> = Vec::with_capacity(jobs.len());
        let mut creators: Vec<String> = Vec::with_capacity(jobs.len());
        let mut subjects: Vec<String> = Vec::with_capacity(jobs.len());
        let mut created_ats: Vec<String> = Vec::with_capacity(jobs.len());
        let mut indexed_ats: Vec<String> = Vec::with_capacity(jobs.len());

        for pj in jobs {
            if let Some(record) = &pj.job.record {
                let uri = pj.uri.to_string();
                let subject = record.get("subject").and_then(|v| v.as_str()).unwrap_or("");
                let created_at = record
                    .get("createdAt")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&pj.job.indexed_at);

                uris.push(uri);
                cids.push(pj.job.cid.clone());
                creators.push(pj.did.clone());
                subjects.push(subject.to_owned());
                created_ats.push(created_at.to_owned());
                indexed_ats.push(pj.job.indexed_at.clone());

                metrics::INDEXER_BLOCK_EVENTS_TOTAL.inc();
            }
        }

        client
            .execute(
                "INSERT INTO actor_block (uri, cid, creator, \"subjectDid\", \"createdAt\", \"indexedAt\")
                 SELECT * FROM unnest($1::text[], $2::text[], $3::text[], $4::text[], $5::text[], $6::text[])
                 ON CONFLICT DO NOTHING",
                &[&uris, &cids, &creators, &subjects, &created_ats, &indexed_ats],
            )
            .await?;

        Ok(())
    }

    async fn batch_insert_profiles(
        client: &deadpool_postgres::Client,
        jobs: &[&ParsedJob<'_>],
    ) -> Result<(), WintermuteError> {
        use crate::metrics;

        if jobs.is_empty() {
            return Ok(());
        }

        let mut uris: Vec<String> = Vec::with_capacity(jobs.len());
        let mut cids: Vec<String> = Vec::with_capacity(jobs.len());
        let mut creators: Vec<String> = Vec::with_capacity(jobs.len());
        let mut display_names: Vec<Option<String>> = Vec::with_capacity(jobs.len());
        let mut descriptions: Vec<Option<String>> = Vec::with_capacity(jobs.len());
        let mut indexed_ats: Vec<String> = Vec::with_capacity(jobs.len());

        for pj in jobs {
            if let Some(record) = &pj.job.record {
                let uri = pj.uri.to_string();
                let display_name = sanitize_opt(record.get("displayName").and_then(|v| v.as_str()));
                let description = sanitize_opt(record.get("description").and_then(|v| v.as_str()));

                uris.push(uri);
                cids.push(pj.job.cid.clone());
                creators.push(pj.did.clone());
                display_names.push(display_name);
                descriptions.push(description);
                indexed_ats.push(pj.job.indexed_at.clone());

                metrics::INDEXER_PROFILE_EVENTS_TOTAL.inc();
            }
        }

        client
            .execute(
                "INSERT INTO profile (uri, cid, creator, \"displayName\", description, \"indexedAt\")
                 SELECT * FROM unnest($1::text[], $2::text[], $3::text[], $4::text[], $5::text[], $6::text[])
                 ON CONFLICT DO NOTHING",
                &[&uris, &cids, &creators, &display_names, &descriptions, &indexed_ats],
            )
            .await?;

        Ok(())
    }

    // Parallel COPY helper functions - each gets its own connection from pool
    // Returns (timing_ms, count, Option<error>) for logging

    async fn parallel_copy_posts(
        pool: &Pool,
        jobs: &[&ParsedJob<'_>],
    ) -> (u128, usize, Option<WintermuteError>) {
        let count = jobs.len();
        if count == 0 {
            return (0, 0, None);
        }
        let start = std::time::Instant::now();
        let client = match pool.get().await {
            Ok(c) => c,
            Err(e) => return (0, count, Some(WintermuteError::Pool(e))),
        };
        let err = Self::copy_batch_insert_posts(&client, jobs).await.err();
        (start.elapsed().as_millis(), count, err)
    }

    async fn parallel_copy_likes(
        pool: &Pool,
        jobs: &[&ParsedJob<'_>],
    ) -> (u128, usize, Option<WintermuteError>) {
        let count = jobs.len();
        if count == 0 {
            return (0, 0, None);
        }
        let start = std::time::Instant::now();

        // Acquire semaphore to serialize like inserts across workers.
        // The like table's 133GB index causes severe contention when
        // multiple workers hit it simultaneously.
        let _permit = LIKE_INSERT_SEMAPHORE.acquire().await;

        let client = match pool.get().await {
            Ok(c) => c,
            Err(e) => return (0, count, Some(WintermuteError::Pool(e))),
        };
        let err = Self::copy_batch_insert_likes(&client, jobs).await.err();
        (start.elapsed().as_millis(), count, err)
    }

    async fn parallel_copy_follows(
        pool: &Pool,
        jobs: &[&ParsedJob<'_>],
    ) -> (u128, usize, Option<WintermuteError>) {
        let count = jobs.len();
        if count == 0 {
            return (0, 0, None);
        }
        let start = std::time::Instant::now();
        let client = match pool.get().await {
            Ok(c) => c,
            Err(e) => return (0, count, Some(WintermuteError::Pool(e))),
        };
        let err = Self::copy_batch_insert_follows(&client, jobs).await.err();
        (start.elapsed().as_millis(), count, err)
    }

    async fn parallel_copy_reposts(
        pool: &Pool,
        jobs: &[&ParsedJob<'_>],
    ) -> (u128, usize, Option<WintermuteError>) {
        let count = jobs.len();
        if count == 0 {
            return (0, 0, None);
        }
        let start = std::time::Instant::now();
        let client = match pool.get().await {
            Ok(c) => c,
            Err(e) => return (0, count, Some(WintermuteError::Pool(e))),
        };
        let err = Self::copy_batch_insert_reposts(&client, jobs).await.err();
        (start.elapsed().as_millis(), count, err)
    }

    async fn parallel_copy_blocks(
        pool: &Pool,
        jobs: &[&ParsedJob<'_>],
    ) -> (u128, usize, Option<WintermuteError>) {
        let count = jobs.len();
        if count == 0 {
            return (0, 0, None);
        }
        let start = std::time::Instant::now();
        let client = match pool.get().await {
            Ok(c) => c,
            Err(e) => return (0, count, Some(WintermuteError::Pool(e))),
        };
        let err = Self::copy_batch_insert_blocks(&client, jobs).await.err();
        (start.elapsed().as_millis(), count, err)
    }

    async fn parallel_copy_profiles(
        pool: &Pool,
        jobs: &[&ParsedJob<'_>],
    ) -> (u128, usize, Option<WintermuteError>) {
        let count = jobs.len();
        if count == 0 {
            return (0, 0, None);
        }
        let start = std::time::Instant::now();
        let client = match pool.get().await {
            Ok(c) => c,
            Err(e) => return (0, count, Some(WintermuteError::Pool(e))),
        };
        let err = Self::batch_insert_profiles(&client, jobs).await.err();
        (start.elapsed().as_millis(), count, err)
    }

    // COPY-based batch insert wrappers that extract data and call bulk functions

    async fn copy_batch_insert_posts(
        client: &deadpool_postgres::Client,
        jobs: &[&ParsedJob<'_>],
    ) -> Result<(), WintermuteError> {
        use crate::metrics;

        if jobs.is_empty() {
            return Ok(());
        }

        let mut post_data: Vec<(String, String, String, String, String, String)> =
            Vec::with_capacity(jobs.len());
        let mut feed_item_data: Vec<(String, String, String, String, String, String)> =
            Vec::with_capacity(jobs.len());

        for pj in jobs {
            if let Some(record) = &pj.job.record {
                let uri = pj.uri.to_string();
                let text = sanitize_text(record.get("text").and_then(|v| v.as_str()).unwrap_or(""));
                let created_at = record
                    .get("createdAt")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&pj.job.indexed_at)
                    .to_owned();
                let sort_at = if pj.job.indexed_at < created_at {
                    pj.job.indexed_at.clone()
                } else {
                    created_at.clone()
                };

                post_data.push((
                    uri.clone(),
                    pj.job.cid.clone(),
                    pj.did.clone(),
                    text,
                    created_at,
                    pj.job.indexed_at.clone(),
                ));

                feed_item_data.push((
                    "post".to_owned(),
                    uri.clone(),
                    pj.job.cid.clone(),
                    uri,
                    pj.did.clone(),
                    sort_at,
                ));

                metrics::INDEXER_POST_EVENTS_TOTAL.inc();
            }
        }

        bulk::copy_insert_posts(client, &post_data).await?;
        bulk::copy_insert_feed_items(client, &feed_item_data).await?;

        Ok(())
    }

    async fn copy_batch_insert_likes(
        client: &deadpool_postgres::Client,
        jobs: &[&ParsedJob<'_>],
    ) -> Result<(), WintermuteError> {
        use crate::metrics;

        if jobs.is_empty() {
            return Ok(());
        }

        let mut like_data: Vec<(String, String, String, String, String, String, String)> =
            Vec::with_capacity(jobs.len());

        for pj in jobs {
            if let Some(record) = &pj.job.record {
                let uri = pj.uri.to_string();
                let subject_obj = record.get("subject");
                let subject_uri = subject_obj
                    .and_then(|s| s.get("uri"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let subject_cid = subject_obj
                    .and_then(|s| s.get("cid"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let created_at = record
                    .get("createdAt")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&pj.job.indexed_at);

                like_data.push((
                    uri,
                    pj.job.cid.clone(),
                    pj.did.clone(),
                    subject_uri.to_owned(),
                    subject_cid.to_owned(),
                    created_at.to_owned(),
                    pj.job.indexed_at.clone(),
                ));

                metrics::INDEXER_LIKE_EVENTS_TOTAL.inc();
            }
        }

        bulk::copy_insert_likes(client, &like_data).await
    }

    async fn copy_batch_insert_follows(
        client: &deadpool_postgres::Client,
        jobs: &[&ParsedJob<'_>],
    ) -> Result<(), WintermuteError> {
        use crate::metrics;

        if jobs.is_empty() {
            return Ok(());
        }

        let mut follow_data: Vec<(String, String, String, String, String, String)> =
            Vec::with_capacity(jobs.len());

        for pj in jobs {
            if let Some(record) = &pj.job.record {
                let uri = pj.uri.to_string();
                let subject = record.get("subject").and_then(|v| v.as_str()).unwrap_or("");
                let created_at = record
                    .get("createdAt")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&pj.job.indexed_at);

                follow_data.push((
                    uri,
                    pj.job.cid.clone(),
                    pj.did.clone(),
                    subject.to_owned(),
                    created_at.to_owned(),
                    pj.job.indexed_at.clone(),
                ));

                metrics::INDEXER_FOLLOW_EVENTS_TOTAL.inc();
            }
        }

        bulk::copy_insert_follows(client, &follow_data).await
    }

    async fn copy_batch_insert_reposts(
        client: &deadpool_postgres::Client,
        jobs: &[&ParsedJob<'_>],
    ) -> Result<(), WintermuteError> {
        use crate::metrics;

        if jobs.is_empty() {
            return Ok(());
        }

        let mut repost_data: Vec<(String, String, String, String, String, String, String)> =
            Vec::with_capacity(jobs.len());
        let mut feed_item_data: Vec<(String, String, String, String, String, String)> =
            Vec::with_capacity(jobs.len());

        for pj in jobs {
            if let Some(record) = &pj.job.record {
                let uri = pj.uri.to_string();
                let subject_obj = record.get("subject");
                let subject_uri = subject_obj
                    .and_then(|s| s.get("uri"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let subject_cid = subject_obj
                    .and_then(|s| s.get("cid"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let created_at = record
                    .get("createdAt")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&pj.job.indexed_at)
                    .to_owned();
                let sort_at = if pj.job.indexed_at < created_at {
                    pj.job.indexed_at.clone()
                } else {
                    created_at.clone()
                };

                repost_data.push((
                    uri.clone(),
                    pj.job.cid.clone(),
                    pj.did.clone(),
                    subject_uri.to_owned(),
                    subject_cid.to_owned(),
                    created_at,
                    pj.job.indexed_at.clone(),
                ));

                feed_item_data.push((
                    "repost".to_owned(),
                    uri,
                    pj.job.cid.clone(),
                    subject_uri.to_owned(),
                    pj.did.clone(),
                    sort_at,
                ));

                metrics::INDEXER_REPOST_EVENTS_TOTAL.inc();
            }
        }

        bulk::copy_insert_reposts(client, &repost_data).await?;
        bulk::copy_insert_feed_items(client, &feed_item_data).await?;

        Ok(())
    }

    async fn copy_batch_insert_blocks(
        client: &deadpool_postgres::Client,
        jobs: &[&ParsedJob<'_>],
    ) -> Result<(), WintermuteError> {
        use crate::metrics;

        if jobs.is_empty() {
            return Ok(());
        }

        let mut block_data: Vec<(String, String, String, String, String, String)> =
            Vec::with_capacity(jobs.len());

        for pj in jobs {
            if let Some(record) = &pj.job.record {
                let uri = pj.uri.to_string();
                let subject = record.get("subject").and_then(|v| v.as_str()).unwrap_or("");
                let created_at = record
                    .get("createdAt")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&pj.job.indexed_at);

                block_data.push((
                    uri,
                    pj.job.cid.clone(),
                    pj.did.clone(),
                    subject.to_owned(),
                    created_at.to_owned(),
                    pj.job.indexed_at.clone(),
                ));

                metrics::INDEXER_BLOCK_EVENTS_TOTAL.inc();
            }
        }

        bulk::copy_insert_blocks(client, &block_data).await
    }

    pub async fn process_label_event(
        pool: &Pool,
        label_event: &crate::types::LabelEvent,
    ) -> Result<(), WintermuteError> {
        use crate::metrics;

        metrics::INDEXER_RECORDS_PROCESSED_TOTAL.inc();

        let client = pool.get().await?;

        // Process each label in the event
        for label in &label_event.labels {
            // Insert or update the label
            // Note: Using empty string for cid since label messages don't include it
            // The primary key is (src, uri, cid, val), so we use "" as cid
            let result = client
                .execute(
                    "INSERT INTO label (src, uri, cid, val, cts, neg)
                     VALUES ($1, $2, $3, $4, $5, false)
                     ON CONFLICT (src, uri, cid, val) DO UPDATE SET
                       cts = EXCLUDED.cts",
                    &[&label.src, &label.uri, &"", &label.val, &label.cts],
                )
                .await;

            match result {
                Ok(_) => {
                    tracing::debug!(
                        "indexed label: src={} uri={} val={}",
                        label.src,
                        label.uri,
                        label.val
                    );
                }
                Err(e) => {
                    tracing::error!("failed to insert label: {e}");
                    metrics::INDEXER_RECORDS_FAILED_TOTAL.inc();
                    // Continue processing other labels even if one fails
                }
            }
        }

        Ok(())
    }

    async fn index_post(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
        record: &serde_json::Value,
        cid: &str,
        indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.feed.post/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        let text = sanitize_text(record.get("text").and_then(|v| v.as_str()).unwrap_or(""));
        let created_at = record
            .get("createdAt")
            .and_then(|v| v.as_str())
            .unwrap_or(indexed_at);

        // Extract reply info
        let reply = record.get("reply");
        let reply_root = reply
            .and_then(|r| r.get("root"))
            .and_then(|r| r.get("uri"))
            .and_then(|v| v.as_str());
        let reply_root_cid = reply
            .and_then(|r| r.get("root"))
            .and_then(|r| r.get("cid"))
            .and_then(|v| v.as_str());
        let reply_parent = reply
            .and_then(|r| r.get("parent"))
            .and_then(|r| r.get("uri"))
            .and_then(|v| v.as_str());
        let reply_parent_cid = reply
            .and_then(|r| r.get("parent"))
            .and_then(|r| r.get("cid"))
            .and_then(|v| v.as_str());

        client
            .execute(
                "INSERT INTO post (uri, cid, creator, text, \"replyRoot\", \"replyRootCid\", \"replyParent\", \"replyParentCid\", \"createdAt\", \"indexedAt\")
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                 ON CONFLICT DO NOTHING",
                &[&uri, &cid, &did, &text, &reply_root, &reply_root_cid, &reply_parent, &reply_parent_cid, &created_at, &indexed_at],
            )
            .await?;

        // sortAt is the earlier of indexedAt and createdAt
        let sort_at = if indexed_at < created_at {
            indexed_at
        } else {
            created_at
        };

        client
            .execute(
                "INSERT INTO feed_item (type, uri, cid, \"postUri\", \"originatorDid\", \"sortAt\")
                 VALUES ('post', $1, $2, $1, $3, $4)
                 ON CONFLICT DO NOTHING",
                &[&uri, &cid, &did, &sort_at],
            )
            .await?;

        client
            .execute(
                "INSERT INTO profile_agg (did, \"postsCount\")
                 VALUES ($1, 1)
                 ON CONFLICT (did) DO UPDATE SET \"postsCount\" = profile_agg.\"postsCount\" + 1",
                &[&did],
            )
            .await?;

        // Generate reply notification for parent post author
        if let Some(parent_uri_str) = reply_parent {
            if let Ok(parent_uri) = AtUri::new(parent_uri_str.to_owned(), None) {
                let parent_author = parent_uri.get_hostname();
                if parent_author != did {
                    client
                        .execute(
                            "INSERT INTO notification (did, author, \"recordUri\", \"recordCid\", reason, \"reasonSubject\", \"sortAt\")
                             VALUES ($1, $2, $3, $4, $5, $6, $7)
                             ON CONFLICT DO NOTHING",
                            &[&parent_author, &did, &uri, &cid, &"reply", &Some(parent_uri_str), &sort_at],
                        )
                        .await?;
                }
            }

            // Update replyCount for parent post
            client
                .execute(
                    "INSERT INTO post_agg (uri, \"replyCount\")
                     VALUES ($1, 1)
                     ON CONFLICT (uri) DO UPDATE SET \"replyCount\" = post_agg.\"replyCount\" + 1",
                    &[&parent_uri_str],
                )
                .await?;
        }

        // Handle embed.record (quote posts)
        if let Some(embed) = record.get("embed") {
            Self::handle_post_embeds(client, embed, &uri, cid, did, created_at, indexed_at).await?;
        }

        Ok(())
    }

    async fn handle_post_embeds(
        client: &deadpool_postgres::Client,
        embed: &serde_json::Value,
        post_uri: &str,
        post_cid: &str,
        creator: &str,
        created_at: &str,
        indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        // Handle app.bsky.embed.record (quote post)
        if let Some(record) = embed.get("record") {
            Self::handle_embed_record(
                client, record, post_uri, post_cid, creator, created_at, indexed_at,
            )
            .await?;
        }

        // Handle app.bsky.embed.recordWithMedia (quote post with media)
        if let Some(record) = embed.get("record").and_then(|r| r.get("record")) {
            Self::handle_embed_record(
                client, record, post_uri, post_cid, creator, created_at, indexed_at,
            )
            .await?;
        }

        Ok(())
    }

    async fn handle_embed_record(
        client: &deadpool_postgres::Client,
        record: &serde_json::Value,
        post_uri: &str,
        post_cid: &str,
        creator: &str,
        created_at: &str,
        indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        let embed_uri = record.get("uri").and_then(|v| v.as_str());
        let embed_cid = record.get("cid").and_then(|v| v.as_str());

        if let (Some(embed_uri), Some(embed_cid)) = (embed_uri, embed_cid) {
            // Only process if it's a post being quoted
            if embed_uri.contains("/app.bsky.feed.post/") {
                // Insert into quote table
                client
                    .execute(
                        "INSERT INTO quote (uri, cid, creator, subject, \"subjectCid\", \"createdAt\", \"indexedAt\")
                         VALUES ($1, $2, $3, $4, $5, $6, $7)
                         ON CONFLICT DO NOTHING",
                        &[&post_uri, &post_cid, &creator, &embed_uri, &embed_cid, &created_at, &indexed_at],
                    )
                    .await?;

                // Update quoteCount for quoted post
                client
                    .execute(
                        "INSERT INTO post_agg (uri, \"quoteCount\")
                         VALUES ($1, 1)
                         ON CONFLICT (uri) DO UPDATE SET \"quoteCount\" = post_agg.\"quoteCount\" + 1",
                        &[&embed_uri],
                    )
                    .await?;

                // Generate quote notification
                if let Ok(quoted_uri) = AtUri::new(embed_uri.to_owned(), None) {
                    let quoted_author = quoted_uri.get_hostname();
                    if quoted_author != creator {
                        let sort_at = if indexed_at < created_at {
                            indexed_at
                        } else {
                            created_at
                        };
                        client
                            .execute(
                                "INSERT INTO notification (did, author, \"recordUri\", \"recordCid\", reason, \"reasonSubject\", \"sortAt\")
                                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                                 ON CONFLICT DO NOTHING",
                                &[&quoted_author, &creator, &post_uri, &post_cid, &"quote", &Some(embed_uri), &sort_at],
                            )
                            .await?;
                    }
                }
            }
        }

        Ok(())
    }

    async fn delete_post(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.feed.post/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();

        // Fetch creator and replyParent before deleting so we can decrement aggregates
        let row = client
            .query_opt(
                "SELECT creator, \"replyParent\" FROM post WHERE uri = $1",
                &[&uri],
            )
            .await?;

        client
            .execute("DELETE FROM post WHERE uri = $1", &[&uri])
            .await?;
        client
            .execute("DELETE FROM feed_item WHERE uri = $1", &[&uri])
            .await?;

        // Decrement aggregate counts based on the deleted post's data
        if let Some(row) = row {
            let creator: Option<String> = row.get("creator");
            let reply_parent: Option<String> = row.get("replyParent");

            if let Some(creator) = creator {
                client
                    .execute(
                        "UPDATE profile_agg SET \"postsCount\" = GREATEST(\"postsCount\" - 1, 0) WHERE did = $1",
                        &[&creator],
                    )
                    .await?;
            }

            if let Some(parent_uri) = reply_parent {
                client
                    .execute(
                        "UPDATE post_agg SET \"replyCount\" = GREATEST(\"replyCount\" - 1, 0) WHERE uri = $1",
                        &[&parent_uri],
                    )
                    .await?;
            }
        }

        Ok(())
    }

    async fn index_like(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
        record: &serde_json::Value,
        cid: &str,
        indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.feed.like/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        let subject = record
            .get("subject")
            .and_then(|v| v.get("uri"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let subject_cid = record
            .get("subject")
            .and_then(|v| v.get("cid"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let created_at = record
            .get("createdAt")
            .and_then(|v| v.as_str())
            .unwrap_or(indexed_at);

        let row_count = client
            .execute(
                "INSERT INTO \"like\" (uri, cid, creator, subject, \"subjectCid\", \"createdAt\", \"indexedAt\")
                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT DO NOTHING",
                &[&uri, &cid, &did, &subject, &subject_cid, &created_at, &indexed_at],
            )
            .await?;

        if row_count > 0 && !subject.is_empty() {
            if let Ok(subject_uri) = AtUri::new(subject.to_owned(), None) {
                let subject_author = subject_uri.get_hostname();
                if subject_author != did {
                    client
                        .execute(
                            "INSERT INTO notification (did, author, \"recordUri\", \"recordCid\", reason, \"reasonSubject\", \"sortAt\")
                             VALUES ($1, $2, $3, $4, $5, $6, $7)",
                            &[&subject_author, &did, &uri, &cid, &"like", &Some(subject), &indexed_at],
                        )
                        .await?;
                }
            }
        }

        if !subject.is_empty() {
            client
                .execute(
                    "INSERT INTO post_agg (uri, \"likeCount\")
                     VALUES ($1, 1)
                     ON CONFLICT (uri) DO UPDATE SET \"likeCount\" = post_agg.\"likeCount\" + 1",
                    &[&subject],
                )
                .await?;
        }

        Ok(())
    }

    async fn delete_like(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.feed.like/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();

        // Fetch subject before deleting so we can decrement likeCount
        let row = client
            .query_opt("SELECT subject FROM \"like\" WHERE uri = $1", &[&uri])
            .await?;

        client
            .execute("DELETE FROM \"like\" WHERE uri = $1", &[&uri])
            .await?;

        if let Some(row) = row {
            let subject: String = row.get("subject");
            if !subject.is_empty() {
                client
                    .execute(
                        "UPDATE post_agg SET \"likeCount\" = GREATEST(\"likeCount\" - 1, 0) WHERE uri = $1",
                        &[&subject],
                    )
                    .await?;
            }
        }

        Ok(())
    }

    async fn index_follow(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
        record: &serde_json::Value,
        cid: &str,
        indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.graph.follow/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        let subject = record.get("subject").and_then(|v| v.as_str()).unwrap_or("");
        let created_at = record
            .get("createdAt")
            .and_then(|v| v.as_str())
            .unwrap_or(indexed_at);

        // Ensure the follow subject also has an actor row (cached)
        if !subject.is_empty() && !ACTOR_CACHE.contains_key(subject) {
            Self::ensure_actor_exists(client, subject, indexed_at).await?;
            if ACTOR_CACHE.len() < ACTOR_CACHE_MAX_SIZE {
                ACTOR_CACHE.insert(subject.to_owned(), ());
            }
        }

        let row_count = client
            .execute(
                "INSERT INTO follow (uri, cid, creator, \"subjectDid\", \"createdAt\", \"indexedAt\")
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT DO NOTHING",
                &[&uri, &cid, &did, &subject, &created_at, &indexed_at],
            )
            .await?;

        if row_count > 0 {
            client
                .execute(
                    "INSERT INTO notification (did, author, \"recordUri\", \"recordCid\", reason, \"reasonSubject\", \"sortAt\")
                     VALUES ($1, $2, $3, $4, $5, $6, $7)",
                    &[&subject, &did, &uri, &cid, &"follow", &None::<String>, &indexed_at],
                )
                .await?;
        }

        client
            .execute(
                "INSERT INTO profile_agg (did, \"followersCount\")
                 VALUES ($1, 1)
                 ON CONFLICT (did) DO UPDATE SET \"followersCount\" = profile_agg.\"followersCount\" + 1",
                &[&subject],
            )
            .await?;

        client
            .execute(
                "INSERT INTO profile_agg (did, \"followsCount\")
                 VALUES ($1, 1)
                 ON CONFLICT (did) DO UPDATE SET \"followsCount\" = profile_agg.\"followsCount\" + 1",
                &[&did],
            )
            .await?;

        Ok(())
    }

    async fn delete_follow(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.graph.follow/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();

        // Fetch creator and subjectDid before deleting so we can decrement aggregate counts
        let row = client
            .query_opt(
                "SELECT creator, \"subjectDid\" FROM follow WHERE uri = $1",
                &[&uri],
            )
            .await?;

        client
            .execute("DELETE FROM follow WHERE uri = $1", &[&uri])
            .await?;

        if let Some(row) = row {
            let creator: String = row.get("creator");
            let subject_did: String = row.get("subjectDid");

            client
                .execute(
                    "UPDATE profile_agg SET \"followsCount\" = GREATEST(\"followsCount\" - 1, 0) WHERE did = $1",
                    &[&creator],
                )
                .await?;

            client
                .execute(
                    "UPDATE profile_agg SET \"followersCount\" = GREATEST(\"followersCount\" - 1, 0) WHERE did = $1",
                    &[&subject_did],
                )
                .await?;
        }

        Ok(())
    }

    async fn index_repost(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
        record: &serde_json::Value,
        cid: &str,
        indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.feed.repost/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        let subject = record
            .get("subject")
            .and_then(|v| v.get("uri"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let subject_cid = record
            .get("subject")
            .and_then(|v| v.get("cid"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let created_at = record
            .get("createdAt")
            .and_then(|v| v.as_str())
            .unwrap_or(indexed_at);

        let row_count = client
            .execute(
                "INSERT INTO repost (uri, cid, creator, subject, \"subjectCid\", \"createdAt\", \"indexedAt\")
                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT DO NOTHING",
                &[&uri, &cid, &did, &subject, &subject_cid, &created_at, &indexed_at],
            )
            .await?;

        // sortAt is the earlier of indexedAt and createdAt
        let sort_at = if indexed_at < created_at {
            indexed_at
        } else {
            created_at
        };

        // feed_item for repost: postUri is the subject (reposted post)
        client
            .execute(
                "INSERT INTO feed_item (type, uri, cid, \"postUri\", \"originatorDid\", \"sortAt\")
                 VALUES ('repost', $1, $2, $3, $4, $5)
                 ON CONFLICT DO NOTHING",
                &[&uri, &cid, &subject, &did, &sort_at],
            )
            .await?;

        if row_count > 0 && !subject.is_empty() {
            if let Ok(subject_uri) = AtUri::new(subject.to_owned(), None) {
                let subject_author = subject_uri.get_hostname();
                if subject_author != did {
                    client
                        .execute(
                            "INSERT INTO notification (did, author, \"recordUri\", \"recordCid\", reason, \"reasonSubject\", \"sortAt\")
                             VALUES ($1, $2, $3, $4, $5, $6, $7)",
                            &[&subject_author, &did, &uri, &cid, &"repost", &Some(subject), &indexed_at],
                        )
                        .await?;
                }
            }
        }

        if !subject.is_empty() {
            client
                .execute(
                    "INSERT INTO post_agg (uri, \"repostCount\")
                     VALUES ($1, 1)
                     ON CONFLICT (uri) DO UPDATE SET \"repostCount\" = post_agg.\"repostCount\" + 1",
                    &[&subject],
                )
                .await?;
        }

        Ok(())
    }

    async fn delete_repost(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.feed.repost/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();

        // Fetch subject before deleting so we can decrement repostCount
        let row = client
            .query_opt("SELECT subject FROM repost WHERE uri = $1", &[&uri])
            .await?;

        client
            .execute("DELETE FROM repost WHERE uri = $1", &[&uri])
            .await?;
        client
            .execute("DELETE FROM feed_item WHERE uri = $1", &[&uri])
            .await?;

        if let Some(row) = row {
            let subject: String = row.get("subject");
            if !subject.is_empty() {
                client
                    .execute(
                        "UPDATE post_agg SET \"repostCount\" = GREATEST(\"repostCount\" - 1, 0) WHERE uri = $1",
                        &[&subject],
                    )
                    .await?;
            }
        }

        Ok(())
    }

    async fn index_block(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
        record: &serde_json::Value,
        cid: &str,
        indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.graph.block/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        let subject = record.get("subject").and_then(|v| v.as_str()).unwrap_or("");
        let created_at = record
            .get("createdAt")
            .and_then(|v| v.as_str())
            .unwrap_or(indexed_at);

        // Ensure the block subject also has an actor row (cached)
        if !subject.is_empty() && !ACTOR_CACHE.contains_key(subject) {
            Self::ensure_actor_exists(client, subject, indexed_at).await?;
            if ACTOR_CACHE.len() < ACTOR_CACHE_MAX_SIZE {
                ACTOR_CACHE.insert(subject.to_owned(), ());
            }
        }

        client
            .execute(
                "INSERT INTO actor_block (uri, cid, creator, \"subjectDid\", \"createdAt\", \"indexedAt\")
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT ON CONSTRAINT actor_block_unique_subject DO NOTHING",
                &[&uri, &cid, &did, &subject, &created_at, &indexed_at],
            )
            .await?;

        Ok(())
    }

    async fn delete_block(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.graph.block/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        client
            .execute("DELETE FROM actor_block WHERE uri = $1", &[&uri])
            .await?;
        Ok(())
    }

    async fn index_profile(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
        record: &serde_json::Value,
        cid: &str,
        indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        if rkey != "self" {
            return Ok(());
        }

        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.actor.profile/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        let display_name = sanitize_opt(record.get("displayName").and_then(|v| v.as_str()));
        let description = sanitize_opt(record.get("description").and_then(|v| v.as_str()));
        let avatar_cid = record
            .get("avatar")
            .and_then(|v| v.get("ref"))
            .and_then(|v| v.get("$link"))
            .and_then(|v| v.as_str());
        let banner_cid = record
            .get("banner")
            .and_then(|v| v.get("ref"))
            .and_then(|v| v.get("$link"))
            .and_then(|v| v.as_str());
        let joined_via_uri = record
            .get("joinedViaStarterPack")
            .and_then(|v| v.get("uri"))
            .and_then(|v| v.as_str());
        let created_at = record
            .get("createdAt")
            .and_then(|v| v.as_str())
            .unwrap_or(indexed_at);

        let row_count = client
            .execute(
                "INSERT INTO profile (uri, cid, creator, \"displayName\", description, \"avatarCid\", \"bannerCid\", \"joinedViaStarterPackUri\", \"createdAt\", \"indexedAt\")
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                 ON CONFLICT DO NOTHING",
                &[&uri, &cid, &did, &display_name, &description, &avatar_cid, &banner_cid, &joined_via_uri, &created_at, &indexed_at],
            )
            .await?;

        if row_count > 0 {
            if let Some(starter_pack_uri_str) = joined_via_uri {
                if let Ok(starter_pack_uri) = AtUri::new(starter_pack_uri_str.to_owned(), None) {
                    let starter_pack_author = starter_pack_uri.get_hostname();
                    client
                        .execute(
                            "INSERT INTO notification (did, author, \"recordUri\", \"recordCid\", reason, \"reasonSubject\", \"sortAt\")
                             VALUES ($1, $2, $3, $4, $5, $6, $7)",
                            &[&starter_pack_author, &did, &uri, &cid, &"starterpack-joined", &Some(starter_pack_uri_str), &indexed_at],
                        )
                        .await?;
                }
            }
        }

        Ok(())
    }

    async fn delete_profile(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.actor.profile/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        client
            .execute("DELETE FROM profile WHERE uri = $1", &[&uri])
            .await?;
        Ok(())
    }

    async fn index_feed_generator(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
        record: &serde_json::Value,
        cid: &str,
        indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.feed.generator/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        let feed_did = record.get("did").and_then(|v| v.as_str());
        let display_name = sanitize_opt(record.get("displayName").and_then(|v| v.as_str()));
        let description = sanitize_opt(record.get("description").and_then(|v| v.as_str()));
        let description_facets = record
            .get("descriptionFacets")
            .map(std::string::ToString::to_string);
        let avatar_cid = record
            .get("avatar")
            .and_then(|v| v.get("ref"))
            .and_then(|v| v.get("$link"))
            .and_then(|v| v.as_str());
        let created_at = record
            .get("createdAt")
            .and_then(|v| v.as_str())
            .unwrap_or(indexed_at);

        client
            .execute(
                "INSERT INTO feed_generator (uri, cid, creator, \"feedDid\", \"displayName\", description, \"descriptionFacets\", \"avatarCid\", \"createdAt\", \"indexedAt\")
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                 ON CONFLICT DO NOTHING",
                &[&uri, &cid, &did, &feed_did, &display_name, &description, &description_facets, &avatar_cid, &created_at, &indexed_at],
            )
            .await?;

        Ok(())
    }

    async fn delete_feed_generator(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.feed.generator/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        client
            .execute("DELETE FROM feed_generator WHERE uri = $1", &[&uri])
            .await?;
        Ok(())
    }

    async fn index_list(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
        record: &serde_json::Value,
        cid: &str,
        indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.graph.list/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        let name = sanitize_opt(record.get("name").and_then(|v| v.as_str()));
        let purpose = record.get("purpose").and_then(|v| v.as_str());
        let description = sanitize_opt(record.get("description").and_then(|v| v.as_str()));
        let description_facets = record
            .get("descriptionFacets")
            .map(std::string::ToString::to_string);
        let avatar_cid = record
            .get("avatar")
            .and_then(|v| v.get("ref"))
            .and_then(|v| v.get("$link"))
            .and_then(|v| v.as_str());
        let created_at = record
            .get("createdAt")
            .and_then(|v| v.as_str())
            .unwrap_or(indexed_at);

        client
            .execute(
                "INSERT INTO list (uri, cid, creator, name, purpose, description, \"descriptionFacets\", \"avatarCid\", \"createdAt\", \"indexedAt\")
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                 ON CONFLICT DO NOTHING",
                &[&uri, &cid, &did, &name, &purpose, &description, &description_facets, &avatar_cid, &created_at, &indexed_at],
            )
            .await?;

        Ok(())
    }

    async fn delete_list(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.graph.list/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        client
            .execute("DELETE FROM list WHERE uri = $1", &[&uri])
            .await?;
        Ok(())
    }

    async fn index_list_item(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
        record: &serde_json::Value,
        cid: &str,
        indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.graph.listitem/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        let list_uri = record.get("list").and_then(|v| v.as_str());
        let subject = record.get("subject").and_then(|v| v.as_str());
        let created_at = record
            .get("createdAt")
            .and_then(|v| v.as_str())
            .unwrap_or(indexed_at);

        // Ensure the list item subject also has an actor row
        if let Some(subj) = subject {
            if !subj.is_empty() {
                Self::ensure_actor_exists(client, subj, indexed_at).await?;
            }
        }

        client
            .execute(
                "INSERT INTO list_item (uri, cid, creator, \"listUri\", \"subjectDid\", \"createdAt\", \"indexedAt\")
                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT DO NOTHING",
                &[&uri, &cid, &did, &list_uri, &subject, &created_at, &indexed_at],
            )
            .await?;

        Ok(())
    }

    async fn delete_list_item(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.graph.listitem/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        client
            .execute("DELETE FROM list_item WHERE uri = $1", &[&uri])
            .await?;
        Ok(())
    }

    async fn index_list_block(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
        record: &serde_json::Value,
        cid: &str,
        indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.graph.listblock/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        let subject = record.get("subject").and_then(|v| v.as_str());
        let created_at = record
            .get("createdAt")
            .and_then(|v| v.as_str())
            .unwrap_or(indexed_at);

        client
            .execute(
                "INSERT INTO list_block (uri, cid, creator, \"subjectUri\", \"createdAt\", \"indexedAt\")
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT DO NOTHING",
                &[&uri, &cid, &did, &subject, &created_at, &indexed_at],
            )
            .await?;

        Ok(())
    }

    async fn delete_list_block(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.graph.listblock/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        client
            .execute("DELETE FROM list_block WHERE uri = $1", &[&uri])
            .await?;
        Ok(())
    }

    async fn index_starter_pack(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
        record: &serde_json::Value,
        cid: &str,
        indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(
            format!("at://{did}/app.bsky.graph.starterpack/{rkey}"),
            None,
        )
        .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        let name = record.get("name").and_then(|v| v.as_str());
        let created_at = record
            .get("createdAt")
            .and_then(|v| v.as_str())
            .unwrap_or(indexed_at);

        client
            .execute(
                "INSERT INTO starter_pack (uri, cid, creator, name, \"createdAt\", \"indexedAt\")
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT DO NOTHING",
                &[&uri, &cid, &did, &name, &created_at, &indexed_at],
            )
            .await?;

        Ok(())
    }

    async fn delete_starter_pack(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(
            format!("at://{did}/app.bsky.graph.starterpack/{rkey}"),
            None,
        )
        .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        client
            .execute("DELETE FROM starter_pack WHERE uri = $1", &[&uri])
            .await?;
        Ok(())
    }

    async fn index_labeler(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
        record: &serde_json::Value,
        cid: &str,
        indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.labeler.service/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        let created_at = record
            .get("createdAt")
            .and_then(|v| v.as_str())
            .unwrap_or(indexed_at);

        client
            .execute(
                "INSERT INTO labeler (uri, cid, creator, \"createdAt\", \"indexedAt\")
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT DO NOTHING",
                &[&uri, &cid, &did, &created_at, &indexed_at],
            )
            .await?;

        Ok(())
    }

    async fn delete_labeler(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.labeler.service/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        client
            .execute("DELETE FROM labeler WHERE uri = $1", &[&uri])
            .await?;
        Ok(())
    }

    async fn index_threadgate(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
        record: &serde_json::Value,
        cid: &str,
        indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.feed.threadgate/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        let post_uri = record.get("post").and_then(|v| v.as_str());
        let created_at = record
            .get("createdAt")
            .and_then(|v| v.as_str())
            .unwrap_or(indexed_at);

        client
            .execute(
                "INSERT INTO thread_gate (uri, cid, creator, \"postUri\", \"createdAt\", \"indexedAt\")
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT DO NOTHING",
                &[&uri, &cid, &did, &post_uri, &created_at, &indexed_at],
            )
            .await?;

        Ok(())
    }

    async fn delete_threadgate(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.feed.threadgate/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        client
            .execute("DELETE FROM thread_gate WHERE uri = $1", &[&uri])
            .await?;
        Ok(())
    }

    async fn index_postgate(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
        record: &serde_json::Value,
        cid: &str,
        indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.feed.postgate/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        let post_uri = record.get("post").and_then(|v| v.as_str());
        let created_at = record
            .get("createdAt")
            .and_then(|v| v.as_str())
            .unwrap_or(indexed_at);

        client
            .execute(
                "INSERT INTO post_gate (uri, cid, creator, \"postUri\", \"createdAt\", \"indexedAt\")
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT DO NOTHING",
                &[&uri, &cid, &did, &post_uri, &created_at, &indexed_at],
            )
            .await?;

        Ok(())
    }

    async fn delete_postgate(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.feed.postgate/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        client
            .execute("DELETE FROM post_gate WHERE uri = $1", &[&uri])
            .await?;
        Ok(())
    }

    #[allow(clippy::unused_async)]
    async fn index_chat_declaration(
        _client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
        _record: &serde_json::Value,
        _cid: &str,
        _indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(
            format!("at://{did}/chat.bsky.actor.declaration/{rkey}"),
            None,
        )
        .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;

        if uri_obj.get_rkey() != "self" {
            return Ok(());
        }

        Ok(())
    }

    #[allow(clippy::unused_async)]
    async fn delete_chat_declaration(
        _client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(
            format!("at://{did}/chat.bsky.actor.declaration/{rkey}"),
            None,
        )
        .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;

        if uri_obj.get_rkey() != "self" {
            return Ok(());
        }

        Ok(())
    }

    #[allow(clippy::unused_async)]
    async fn index_notif_declaration(
        _client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
        _record: &serde_json::Value,
        _cid: &str,
        _indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        // Placeholder - this record type is only indexed in the generic record table
        let uri_obj = AtUri::new(
            format!("at://{did}/app.bsky.notification.declaration/{rkey}"),
            None,
        )
        .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;

        if uri_obj.get_rkey() != "self" {
            return Ok(());
        }

        Ok(())
    }

    #[allow(clippy::unused_async)]
    async fn delete_notif_declaration(
        _client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
    ) -> Result<(), WintermuteError> {
        // Placeholder - this record type is only indexed in the generic record table
        let uri_obj = AtUri::new(
            format!("at://{did}/app.bsky.notification.declaration/{rkey}"),
            None,
        )
        .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;

        if uri_obj.get_rkey() != "self" {
            return Ok(());
        }

        Ok(())
    }

    #[allow(clippy::unused_async)]
    async fn index_status(
        _client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
        _record: &serde_json::Value,
        _cid: &str,
        _indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        // Placeholder - app.bsky.actor.status table doesn't exist in production schema
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.actor.status/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;

        if uri_obj.get_rkey() != "self" {
            return Ok(());
        }

        Ok(())
    }

    #[allow(clippy::unused_async)]
    async fn delete_status(
        _client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
    ) -> Result<(), WintermuteError> {
        // Placeholder - app.bsky.actor.status table doesn't exist in production schema
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.actor.status/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;

        if uri_obj.get_rkey() != "self" {
            return Ok(());
        }

        Ok(())
    }

    async fn index_verification(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
        record: &serde_json::Value,
        cid: &str,
        indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(
            format!("at://{did}/app.bsky.verification.proof/{rkey}"),
            None,
        )
        .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();

        // Extract fields from the verification record
        let subject = record.get("subject").and_then(|v| v.as_str()).unwrap_or("");
        let handle = record.get("handle").and_then(|v| v.as_str()).unwrap_or("");
        let display_name = sanitize_text(
            record
                .get("displayName")
                .and_then(|v| v.as_str())
                .unwrap_or(""),
        );
        let created_at = record
            .get("createdAt")
            .and_then(|v| v.as_str())
            .unwrap_or(indexed_at);

        client
            .execute(
                "INSERT INTO verification (uri, cid, rkey, creator, subject, handle, \"displayName\", \"createdAt\", \"indexedAt\")
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                 ON CONFLICT DO NOTHING",
                &[&uri, &cid, &rkey, &did, &subject, &handle, &display_name, &created_at, &indexed_at],
            )
            .await?;

        Ok(())
    }

    async fn delete_verification(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(
            format!("at://{did}/app.bsky.verification.proof/{rkey}"),
            None,
        )
        .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        client
            .execute("DELETE FROM verification WHERE uri = $1", &[&uri])
            .await?;
        Ok(())
    }
}
