mod tests;

use crate::SHUTDOWN;
#[cfg(test)]
use crate::config::INDEXER_BATCH_SIZE;
use crate::config::{
    DB_POOL_SIZE, HANDLE_REINDEX_INTERVAL_INVALID, HANDLE_REINDEX_INTERVAL_VALID,
    IDENTITY_RESOLVER_TIMEOUT, WORKERS_INDEXER,
};
use crate::storage::Storage;
#[cfg(test)]
use crate::types::LabelEvent;
use crate::types::{IndexJob, WintermuteError, WriteAction};
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
    pool: Pool,
    semaphore_backfill: Arc<Semaphore>,
    id_resolver: Arc<Mutex<IdResolver>>,
}

impl IndexerManager {
    pub fn new(storage: Arc<Storage>, database_url: String) -> Result<Self, WintermuteError> {
        let pool_size = *DB_POOL_SIZE;
        tracing::info!("indexer DB pool size: {pool_size}");
        let mut pg_config = Config::new();
        pg_config.url = Some(database_url);
        pg_config.manager = Some(ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        });
        pg_config.pool = Some(deadpool_postgres::PoolConfig::new(pool_size));

        let pool = pg_config
            .create_pool(Some(Runtime::Tokio1), NoTls)
            .map_err(|e| WintermuteError::Other(format!("pool creation failed: {e}")))?;

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
            pool,
            // Only backfill gets semaphore; firehose_live and label_live are unbounded
            semaphore_backfill: Arc::new(Semaphore::new(workers)),
            id_resolver: Arc::new(Mutex::new(id_resolver)),
        })
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
        tracing::info!("handle resolution processor started");
        let mut batch_count = 0u64;

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
            if batch_count % 10 == 1 {
                tracing::info!(
                    "handle resolution batch {}, {} actors",
                    batch_count,
                    dids_to_resolve.len()
                );
            }

            let timestamp = chrono::Utc::now().to_rfc3339();
            let client = match self.pool.get().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("failed to get db connection: {e}");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }
            };

            let mut resolved_count = 0;
            for did in dids_to_resolve {
                match Self::index_handle(&client, &self.id_resolver, &did, &timestamp, false).await
                {
                    Ok(true) => resolved_count += 1,
                    Ok(false) => {}
                    Err(e) => tracing::debug!("handle resolution error for {did}: {e}"),
                }

                // Rate limit: 100ms between resolutions to avoid hammering DID resolvers
                tokio::time::sleep(Duration::from_millis(100)).await;
            }

            if resolved_count > 0 {
                tracing::info!("resolved {resolved_count} handles in batch {batch_count}");
            }
        }
    }

    async fn get_actors_needing_handle_resolution(&self) -> Result<Vec<String>, WintermuteError> {
        let client = self.pool.get().await?;

        // Get actors with NULL handle or stale indexedAt
        // Priority: NULL handles first, then stale valid handles
        // RFC3339 strings are lexicographically sortable, so we compare as text
        let stale_threshold_invalid = (chrono::Utc::now()
            - chrono::Duration::from_std(HANDLE_REINDEX_INTERVAL_INVALID).unwrap_or_default())
        .to_rfc3339();
        let stale_threshold_valid = (chrono::Utc::now()
            - chrono::Duration::from_std(HANDLE_REINDEX_INTERVAL_VALID).unwrap_or_default())
        .to_rfc3339();

        let rows = client
            .query(
                "SELECT did FROM actor
                 WHERE (handle IS NULL AND \"indexedAt\" < $1)
                    OR (handle IS NOT NULL AND \"indexedAt\" < $2)
                 ORDER BY
                   CASE WHEN handle IS NULL THEN 0 ELSE 1 END,
                   \"indexedAt\" ASC
                 LIMIT 100",
                &[&stale_threshold_invalid, &stale_threshold_valid],
            )
            .await?;

        let dids: Vec<String> = rows.iter().map(|row| row.get("did")).collect();
        Ok(dids)
    }

    async fn process_firehose_live_loop(&self) {
        const MAX_CONCURRENT: usize = 100;

        tracing::info!("firehose_live processor started (unbounded concurrency)");
        let mut in_flight: FuturesUnordered<JobTaskHandle> = FuturesUnordered::new();
        let mut processed_count = 0u64;
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
            while in_flight.len() < MAX_CONCURRENT {
                if SHUTDOWN.load(Ordering::Relaxed) {
                    break;
                }
                match self.storage.dequeue_firehose_live() {
                    Ok(Some((key, job))) => {
                        let pool = self.pool.clone();
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
            } else if in_flight.len() >= MAX_CONCURRENT {
                // At capacity - wait for one to complete
                if let Some(result) = in_flight.next().await {
                    self.handle_single_job_result(result);
                    processed_count += 1;
                }
            } else {
                tokio::task::yield_now().await;
            }

            // Log progress periodically
            if last_log.elapsed() > Duration::from_secs(5) {
                tracing::info!(
                    "firehose_live: processed {}, in_flight {}",
                    processed_count,
                    in_flight.len()
                );
                last_log = std::time::Instant::now();
            }
        }
    }

    async fn process_firehose_backfill_loop(&self) {
        tracing::info!("firehose_backfill processor started");
        let mut in_flight: FuturesUnordered<JobTaskHandle> = FuturesUnordered::new();
        let mut processed_count = 0u64;
        let mut last_log = std::time::Instant::now();

        loop {
            if SHUTDOWN.load(Ordering::Relaxed) {
                tracing::info!(
                    "shutdown requested for firehose_backfill processor, draining {} in-flight tasks",
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
                        "firehose_backfill: {} tasks still in-flight after drain timeout",
                        in_flight.len()
                    );
                }
                break;
            }

            // Try to fill up to semaphore capacity with new jobs
            while self.semaphore_backfill.available_permits() > 0 {
                if SHUTDOWN.load(Ordering::Relaxed) {
                    break;
                }
                match self.storage.dequeue_firehose_backfill() {
                    Ok(Some((key, job))) => {
                        if let Ok(permit) = self.semaphore_backfill.clone().try_acquire_owned() {
                            let pool = self.pool.clone();
                            let task = tokio::spawn(async move {
                                let result = Self::process_job(&pool, &job).await;
                                drop(permit);
                                (key, QueueSource::FirehoseBackfill, result)
                            });
                            in_flight.push(task);
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        tracing::error!("failed to dequeue firehose_backfill job: {e}");
                        break;
                    }
                }
            }

            // Process completed tasks or wait briefly if nothing to do
            if in_flight.is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            } else {
                tokio::select! {
                    Some(result) = in_flight.next() => {
                        self.handle_single_job_result(result);
                        processed_count += 1;
                    }
                    () = tokio::time::sleep(Duration::from_millis(1)) => {}
                }
            }

            // Log progress periodically
            if last_log.elapsed() > Duration::from_secs(5) {
                tracing::info!(
                    "firehose_backfill: processed {}, in_flight {}",
                    processed_count,
                    in_flight.len()
                );
                last_log = std::time::Instant::now();
            }
        }
    }

    async fn process_labels_loop(&self) {
        const MAX_CONCURRENT: usize = 100;

        tracing::info!("label_live processor started (unbounded concurrency)");
        let mut in_flight: FuturesUnordered<LabelTaskHandle> = FuturesUnordered::new();
        let mut processed_count = 0u64;
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
            while in_flight.len() < MAX_CONCURRENT {
                if SHUTDOWN.load(Ordering::Relaxed) {
                    break;
                }
                match self.storage.dequeue_label_live() {
                    Ok(Some((key, label_event))) => {
                        let pool = self.pool.clone();
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
            } else if in_flight.len() >= MAX_CONCURRENT {
                // At capacity - wait for one to complete
                if let Some(result) = in_flight.next().await {
                    self.handle_single_label_result(result);
                    processed_count += 1;
                }
            } else {
                tokio::task::yield_now().await;
            }

            // Log progress periodically
            if last_log.elapsed() > Duration::from_secs(5) {
                tracing::info!(
                    "label_live: processed {}, in_flight {}",
                    processed_count,
                    in_flight.len()
                );
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

            let pool = self.pool.clone();

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
        let result = client
            .query_opt(
                "UPDATE record
                 SET rev = $2, json = '', cid = ''
                 WHERE uri = $1 AND rev <= $2
                 RETURNING uri",
                &[&uri, &rev],
            )
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

                // Ensure actor row exists for this DID
                Self::ensure_actor_exists(&client, did.as_str(), &job.indexed_at).await?;

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

        client
            .execute(
                "INSERT INTO post (uri, cid, creator, text, \"createdAt\", \"indexedAt\")
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT DO NOTHING",
                &[&uri, &cid, &did, &text, &created_at, &indexed_at],
            )
            .await?;

        client
            .execute(
                "INSERT INTO profile_agg (did, \"postsCount\")
                 SELECT $1::varchar, COUNT(*) FROM post WHERE creator = $1
                 ON CONFLICT (did) DO UPDATE SET \"postsCount\" = EXCLUDED.\"postsCount\"",
                &[&did],
            )
            .await?;

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
        client
            .execute("DELETE FROM post WHERE uri = $1", &[&uri])
            .await?;
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
                     SELECT $1::varchar, COUNT(*) FROM \"like\" WHERE subject = $1
                     ON CONFLICT (uri) DO UPDATE SET \"likeCount\" = EXCLUDED.\"likeCount\"",
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
        client
            .execute("DELETE FROM \"like\" WHERE uri = $1", &[&uri])
            .await?;
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

        // Ensure the follow subject also has an actor row
        if !subject.is_empty() {
            Self::ensure_actor_exists(client, subject, indexed_at).await?;
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
                 SELECT $1::varchar, COUNT(*) FROM follow WHERE \"subjectDid\" = $1
                 ON CONFLICT (did) DO UPDATE SET \"followersCount\" = EXCLUDED.\"followersCount\"",
                &[&subject],
            )
            .await?;

        client
            .execute(
                "INSERT INTO profile_agg (did, \"followsCount\")
                 SELECT $1::varchar, COUNT(*) FROM follow WHERE creator = $1
                 ON CONFLICT (did) DO UPDATE SET \"followsCount\" = EXCLUDED.\"followsCount\"",
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
        client
            .execute("DELETE FROM follow WHERE uri = $1", &[&uri])
            .await?;
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
                     SELECT $1::varchar, COUNT(*) FROM repost WHERE subject = $1
                     ON CONFLICT (uri) DO UPDATE SET \"repostCount\" = EXCLUDED.\"repostCount\"",
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
        client
            .execute("DELETE FROM repost WHERE uri = $1", &[&uri])
            .await?;
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

        // Ensure the block subject also has an actor row
        if !subject.is_empty() {
            Self::ensure_actor_exists(client, subject, indexed_at).await?;
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
            .and_then(|v| v.as_str());
        let banner_cid = record
            .get("banner")
            .and_then(|v| v.get("ref"))
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
