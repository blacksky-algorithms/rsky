mod tests;

use crate::SHUTDOWN;
use crate::config::{
    BACKFILLER_BATCH_SIZE, BACKFILLER_OUTPUT_HIGH_WATER_MARK, WORKERS_BACKFILLER,
    backfiller_timeout,
};
use crate::storage::Storage;
use crate::types::{BackfillJob, IndexJob, WintermuteError, WriteAction};
use iroh_car::CarReader;
use rsky_identity::IdResolver;
use rsky_identity::types::IdentityResolverOpts;
use rsky_repo::parse::get_and_parse_record;
use rsky_repo::readable_repo::ReadableRepo;
use rsky_repo::storage::memory_blockstore::MemoryBlockstore;
use rsky_syntax::aturi::AtUri;
use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::Semaphore;

// Type alias to reduce complexity
type BackfillTaskResult = (Vec<u8>, BackfillJob, Result<(), WintermuteError>);

pub struct BackfillerManager {
    workers: usize,
    storage: Arc<Storage>,
    http_client: reqwest::Client,
    semaphore: Arc<Semaphore>,
}

impl BackfillerManager {
    pub fn new(storage: Arc<Storage>) -> Result<Self, WintermuteError> {
        let workers = *WORKERS_BACKFILLER;
        let http_client = reqwest::Client::builder()
            .timeout(backfiller_timeout())
            .build()?;

        tracing::info!(
            "backfiller config: workers={}, batch_size={}, high_water_mark={}, timeout={:?}",
            workers,
            *BACKFILLER_BATCH_SIZE,
            *BACKFILLER_OUTPUT_HIGH_WATER_MARK,
            backfiller_timeout()
        );

        Ok(Self {
            workers,
            storage,
            http_client,
            semaphore: Arc::new(Semaphore::new(workers)),
        })
    }

    pub fn run(self) -> Result<(), WintermuteError> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(self.workers)
            .enable_all()
            .build()
            .map_err(|e| WintermuteError::Other(format!("failed to create runtime: {e}")))?;

        rt.block_on(async {
            self.process_loop().await;
        });

        Ok(())
    }

    async fn process_loop(&self) {
        // Exponential backoff for empty queue: 100ms -> 200ms -> 400ms -> ... -> 5s max
        const MAX_EMPTY_BACKOFF_MS: u64 = 5000;
        let mut empty_backoff_ms = 100u64;

        loop {
            if SHUTDOWN.load(Ordering::Relaxed) {
                tracing::info!("shutdown requested for backfiller");
                break;
            }

            self.update_metrics();

            if self.check_backpressure().await {
                continue;
            }

            let jobs = self.dequeue_batch();
            if jobs.is_empty() {
                tokio::time::sleep(Duration::from_millis(empty_backoff_ms)).await;
                empty_backoff_ms = (empty_backoff_ms * 2).min(MAX_EMPTY_BACKOFF_MS);
                continue;
            }

            // Reset backoff when we have work
            empty_backoff_ms = 100;

            let tasks = self.spawn_job_tasks(jobs).await;
            self.handle_task_results(tasks).await;
        }
    }

    fn update_metrics(&self) {
        if let Ok(queue_len) = self.storage.repo_backfill_len() {
            crate::metrics::BACKFILLER_REPOS_WAITING
                .set(i64::try_from(queue_len).unwrap_or(i64::MAX));
        }
    }

    async fn check_backpressure(&self) -> bool {
        let high_water_mark = *BACKFILLER_OUTPUT_HIGH_WATER_MARK;
        if let Ok(output_len) = self.storage.firehose_backfill_len() {
            crate::metrics::BACKFILLER_OUTPUT_STREAM_LENGTH
                .set(i64::try_from(output_len).unwrap_or(i64::MAX));

            if output_len >= high_water_mark {
                crate::metrics::BACKFILLER_BACKPRESSURE_EVENTS_TOTAL.inc();
                tracing::warn!(
                    "backpressure: output stream at {output_len}, pausing (high water mark: {high_water_mark})"
                );
                tokio::time::sleep(Duration::from_secs(1)).await;
                return true;
            }
        }
        false
    }

    fn dequeue_batch(&self) -> Vec<(Vec<u8>, BackfillJob)> {
        let batch_size = *BACKFILLER_BATCH_SIZE;
        let mut jobs = Vec::with_capacity(batch_size);
        for _ in 0..batch_size {
            match self.storage.dequeue_backfill() {
                Ok(Some((key, job))) => jobs.push((key, job)),
                Ok(None) => break,
                Err(e) => {
                    tracing::error!("failed to dequeue backfill job: {e}");
                    break;
                }
            }
        }
        jobs
    }

    async fn spawn_job_tasks(
        &self,
        jobs: Vec<(Vec<u8>, BackfillJob)>,
    ) -> Vec<tokio::task::JoinHandle<(Vec<u8>, BackfillJob, Result<(), WintermuteError>)>> {
        let mut tasks = Vec::new();
        for (key, job) in jobs {
            let Ok(permit) = self.semaphore.clone().acquire_owned().await else {
                break;
            };

            let storage = Arc::clone(&self.storage);
            let http_client = self.http_client.clone();

            let task = tokio::spawn(async move {
                let result = Self::process_job(&storage, &http_client, &job).await;
                drop(permit);
                (key, job, result)
            });

            tasks.push(task);
        }
        tasks
    }

    async fn handle_task_results(&self, tasks: Vec<tokio::task::JoinHandle<BackfillTaskResult>>) {
        for task in tasks {
            match task.await {
                Ok((key, _job, Ok(()))) => {
                    if let Err(e) = self.storage.remove_backfill(&key) {
                        tracing::error!("failed to remove backfill job: {e}");
                    }
                }
                Ok((key, mut job, Err(e))) => {
                    tracing::error!("backfill job failed for {}: {e}", job.did);
                    job.retry_count += 1;
                    if job.retry_count < 3 {
                        crate::metrics::BACKFILLER_RETRIES_ATTEMPTED_TOTAL.inc();
                        drop(self.storage.remove_backfill(&key));
                        drop(self.storage.enqueue_backfill(&job));
                    } else {
                        tracing::error!("backfill job exceeded retries: {}", job.did);
                        crate::metrics::BACKFILLER_REPOS_DEAD_LETTERED_TOTAL.inc();
                        drop(self.storage.remove_backfill(&key));
                    }
                }
                Err(e) => {
                    tracing::error!("task panicked: {e}");
                }
            }
        }
    }

    pub async fn process_job(
        storage: &Storage,
        http_client: &reqwest::Client,
        job: &BackfillJob,
    ) -> Result<(), WintermuteError> {
        use crate::metrics;

        metrics::BACKFILLER_REPOS_RUNNING.inc();

        let did = &job.did;

        let resolver_opts = IdentityResolverOpts {
            timeout: None,
            plc_url: None,
            did_cache: None,
            backup_nameservers: None,
        };
        let mut resolver = IdResolver::new(resolver_opts);
        let Ok(Some(doc)) = resolver.did.resolve(did.to_string(), None).await else {
            metrics::BACKFILLER_REPOS_FAILED_TOTAL.inc();
            metrics::BACKFILLER_REPOS_RUNNING.dec();
            return Err(WintermuteError::Other(format!(
                "did resolution failed for: {did}"
            )));
        };

        let mut pds_endpoint = None;
        if let Some(services) = &doc.service {
            for service in services {
                if service.r#type == "AtprotoPersonalDataServer" || service.id == "#atproto_pds" {
                    pds_endpoint = Some(service.service_endpoint.clone());
                    break;
                }
            }
        }

        let Some(pds_endpoint) = pds_endpoint else {
            metrics::BACKFILLER_REPOS_FAILED_TOTAL.inc();
            metrics::BACKFILLER_REPOS_RUNNING.dec();
            return Err(WintermuteError::Other(format!("no pds found: {did}")));
        };

        let repo_url = format!("{pds_endpoint}/xrpc/com.atproto.sync.getRepo?did={did}");
        let response = match http_client.get(&repo_url).send().await {
            Ok(r) => r,
            Err(e) => {
                metrics::BACKFILLER_CAR_FETCH_ERRORS_TOTAL.inc();
                metrics::BACKFILLER_REPOS_FAILED_TOTAL.inc();
                metrics::BACKFILLER_REPOS_RUNNING.dec();
                return Err(WintermuteError::Other(format!("http error: {e}")));
            }
        };

        if !response.status().is_success() {
            metrics::BACKFILLER_CAR_FETCH_ERRORS_TOTAL.inc();
            metrics::BACKFILLER_REPOS_FAILED_TOTAL.inc();
            metrics::BACKFILLER_REPOS_RUNNING.dec();
            return Err(WintermuteError::Other(format!(
                "http error: {}",
                response.status()
            )));
        }

        let car_bytes = response.bytes().await?;
        let mut reader = match CarReader::new(Cursor::new(car_bytes.to_vec())).await {
            Ok(r) => r,
            Err(e) => {
                metrics::BACKFILLER_CAR_PARSE_ERRORS_TOTAL.inc();
                metrics::BACKFILLER_REPOS_FAILED_TOTAL.inc();
                metrics::BACKFILLER_REPOS_RUNNING.dec();
                return Err(WintermuteError::Repo(format!("car read failed: {e}")));
            }
        };

        let root = *reader
            .header()
            .roots()
            .first()
            .ok_or_else(|| WintermuteError::Repo("no root cid".into()))?;

        let mut blocks = rsky_repo::block_map::BlockMap::new();
        while let Some((cid, data)) = reader
            .next_block()
            .await
            .map_err(|e| WintermuteError::Repo(format!("read block failed: {e}")))?
        {
            blocks.set(cid, data.clone());
        }

        let blockstore = MemoryBlockstore::new(Some(blocks))
            .await
            .map_err(|e| WintermuteError::Repo(format!("blockstore failed: {e}")))?;
        let storage_arc = Arc::new(tokio::sync::RwLock::new(blockstore));

        let mut repo = ReadableRepo::load(storage_arc, root)
            .await
            .map_err(|e| WintermuteError::Repo(format!("repo load failed: {e}")))?;

        if repo.did() != did {
            metrics::BACKFILLER_VERIFICATION_ERRORS_TOTAL.inc();
            metrics::BACKFILLER_REPOS_FAILED_TOTAL.inc();
            metrics::BACKFILLER_REPOS_RUNNING.dec();
            return Err(WintermuteError::Repo(format!(
                "did mismatch: expected {did}, got {}",
                repo.did()
            )));
        }

        let leaves = repo
            .data
            .list(None, None, None)
            .await
            .map_err(|e| WintermuteError::Repo(format!("list failed: {e}")))?;

        let blocks_result = {
            let storage_guard = repo.storage.read().await;
            storage_guard
                .get_blocks(leaves.iter().map(|e| e.value).collect())
                .await
                .map_err(|e| WintermuteError::Repo(format!("get blocks failed: {e}")))?
        };

        let rev = repo.commit.rev.clone();
        let now = chrono::Utc::now().to_rfc3339();

        for entry in &leaves {
            let uri_string = format!("at://{did}/{}", entry.key);
            let Ok(uri) = AtUri::new(uri_string, None) else {
                continue;
            };

            let collection = uri.get_collection();
            let rkey = uri.get_rkey();

            if !collection.starts_with("app.bsky.") && !collection.starts_with("chat.bsky.") {
                metrics::BACKFILLER_RECORDS_FILTERED_TOTAL.inc();
                continue;
            }

            if let Ok(parsed) = get_and_parse_record(&blocks_result.blocks, entry.value) {
                metrics::BACKFILLER_RECORDS_EXTRACTED_TOTAL.inc();
                let record_json_raw = serde_json::to_value(&parsed.record)
                    .map_err(|e| WintermuteError::Serialization(format!("json failed: {e}")))?;
                let record_json = convert_record_to_ipld(&record_json_raw);

                let uri_string = format!("at://{did}/{collection}/{rkey}");
                let uri = AtUri::new(uri_string, None)
                    .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
                let cid = entry.value.to_string();

                let index_job = IndexJob {
                    uri: uri.to_string(),
                    cid,
                    action: WriteAction::Create,
                    record: Some(record_json),
                    indexed_at: now.clone(),
                    rev: rev.clone(),
                };

                if job.priority {
                    storage.enqueue_firehose_backfill_priority(&index_job)?;
                } else {
                    storage.enqueue_firehose_backfill(&index_job)?;
                }
            }
        }

        metrics::BACKFILLER_REPOS_PROCESSED_TOTAL.inc();
        metrics::BACKFILLER_REPOS_RUNNING.dec();

        Ok(())
    }
}

pub fn convert_record_to_ipld(record_json: &serde_json::Value) -> serde_json::Value {
    match record_json {
        serde_json::Value::Object(map) => {
            let mut new_map = serde_json::Map::new();
            for (k, v) in map {
                new_map.insert(k.clone(), convert_record_to_ipld(v));
            }
            serde_json::Value::Object(new_map)
        }
        serde_json::Value::Array(arr) => {
            let is_byte_array = arr.iter().all(|v| {
                matches!(v, serde_json::Value::Number(n) if n.as_u64().is_some_and(|num| num <= 255))
            });

            if is_byte_array && !arr.is_empty() {
                let bytes: Vec<u8> = arr
                    .iter()
                    .filter_map(|v| v.as_u64().and_then(|n| u8::try_from(n).ok()))
                    .collect();

                if let Ok(cid) = lexicon_cid::Cid::try_from(&bytes[..]) {
                    return serde_json::json!({"$link": cid.to_string()});
                }
            }

            serde_json::Value::Array(arr.iter().map(convert_record_to_ipld).collect())
        }
        other => other.clone(),
    }
}
