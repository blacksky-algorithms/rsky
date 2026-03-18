mod tests;

use crate::SHUTDOWN;
use crate::config::{BACKFILLER_BATCH_SIZE, WORKERS_BACKFILLER, backfiller_timeout};
use crate::storage::Storage;
use crate::types::{BackfillJob, IndexJob, WintermuteError, WriteAction};
use dashmap::DashMap;
use iroh_car::CarReader;
use rsky_identity::IdResolver;
use rsky_identity::types::IdentityResolverOpts;
use rsky_repo::parse::get_and_parse_record;
use rsky_repo::readable_repo::ReadableRepo;
use rsky_repo::storage::memory_blockstore::MemoryBlockstore;
use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

pub struct BackfillerManager {
    workers: usize,
    storage: Arc<Storage>,
    http_client: reqwest::Client,
    pds_cache: Arc<DashMap<String, String>>,
}

impl BackfillerManager {
    pub fn new(storage: Arc<Storage>) -> Result<Self, WintermuteError> {
        let workers = *WORKERS_BACKFILLER;
        let http_client = reqwest::Client::builder()
            .timeout(backfiller_timeout())
            .build()?;

        tracing::info!(
            "backfiller config: workers={}, batch_size={}, timeout={:?}",
            workers,
            *BACKFILLER_BATCH_SIZE,
            backfiller_timeout()
        );

        Ok(Self {
            workers,
            storage,
            http_client,
            pds_cache: Arc::new(DashMap::new()),
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

    /// Continuous pipeline: a dequeue task feeds jobs into a channel,
    /// N worker tasks consume from the channel and process repos independently.
    /// No batch barriers -- each worker immediately picks up the next job when done.
    async fn process_loop(&self) {
        const MAX_EMPTY_BACKOFF_MS: u64 = 5000;
        let (tx, rx) = tokio::sync::mpsc::channel::<(Vec<u8>, BackfillJob)>(*BACKFILLER_BATCH_SIZE);
        let rx = Arc::new(tokio::sync::Mutex::new(rx));

        tracing::info!(
            "backfiller starting continuous pipeline with {} workers",
            self.workers
        );

        // Spawn worker tasks -- each loops forever, pulling from the channel
        let mut worker_handles = Vec::with_capacity(self.workers);
        for worker_id in 0..self.workers {
            let rx = Arc::clone(&rx);
            let storage = Arc::clone(&self.storage);
            let http_client = self.http_client.clone();
            let pds_cache = Arc::clone(&self.pds_cache);

            worker_handles.push(tokio::spawn(async move {
                loop {
                    // Acquire the next job from the channel
                    let job_opt = {
                        let mut guard = rx.lock().await;
                        guard.recv().await
                    };

                    let Some((_key, job)) = job_opt else {
                        tracing::info!("worker {worker_id}: channel closed, exiting");
                        break;
                    };

                    match Self::process_job(&storage, &http_client, &pds_cache, &job).await {
                        Ok(()) => {}
                        Err(e) => {
                            tracing::error!("worker {worker_id}: failed {}: {e}", job.did);
                            if job.retry_count < 2 {
                                let mut retry_job = job;
                                retry_job.retry_count += 1;
                                crate::metrics::BACKFILLER_RETRIES_ATTEMPTED_TOTAL.inc();
                                drop(storage.enqueue_backfill(&retry_job));
                            } else {
                                tracing::error!(
                                    "worker {worker_id}: exceeded retries: {}",
                                    job.did
                                );
                                crate::metrics::BACKFILLER_REPOS_DEAD_LETTERED_TOTAL.inc();
                            }
                        }
                    }
                }
            }));
        }

        // Dequeue task: continuously feed the channel from Fjall
        let dequeue_storage = Arc::clone(&self.storage);
        let dequeue_handle = tokio::spawn(async move {
            let mut empty_backoff_ms = 100u64;

            loop {
                if SHUTDOWN.load(Ordering::Relaxed) {
                    tracing::info!("dequeue task: shutdown requested");
                    break;
                }

                let batch_size = *BACKFILLER_BATCH_SIZE;
                let jobs = match dequeue_storage.dequeue_backfill_batch(batch_size) {
                    Ok(jobs) => jobs,
                    Err(e) => {
                        tracing::error!("dequeue task: failed to dequeue: {e}");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                };

                if jobs.is_empty() {
                    tokio::time::sleep(Duration::from_millis(empty_backoff_ms)).await;
                    empty_backoff_ms = (empty_backoff_ms * 2).min(MAX_EMPTY_BACKOFF_MS);
                    continue;
                }

                empty_backoff_ms = 100;
                let count = jobs.len();

                for job in jobs {
                    if tx.send(job).await.is_err() {
                        tracing::info!("dequeue task: channel closed, exiting");
                        return;
                    }
                }

                tracing::debug!("dequeue task: sent {count} jobs to workers");
            }

            // Drop sender to signal workers to exit
            drop(tx);
        });

        // Wait for dequeue task to finish (on shutdown)
        drop(dequeue_handle.await);

        // Workers will exit when channel is closed
        for handle in worker_handles {
            drop(handle.await);
        }

        tracing::info!("backfiller pipeline stopped");
    }

    pub async fn process_job(
        storage: &Storage,
        http_client: &reqwest::Client,
        pds_cache: &DashMap<String, String>,
        job: &BackfillJob,
    ) -> Result<(), WintermuteError> {
        use crate::metrics;

        metrics::BACKFILLER_REPOS_RUNNING.inc();

        let did = &job.did;

        // Check PDS endpoint cache first to avoid repeated DID resolution
        let pds_endpoint = if let Some(cached) = pds_cache.get(did) {
            cached.value().clone()
        } else {
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

            let mut endpoint = None;
            if let Some(services) = &doc.service {
                for service in services {
                    if service.r#type == "AtprotoPersonalDataServer" || service.id == "#atproto_pds"
                    {
                        endpoint = Some(service.service_endpoint.clone());
                        break;
                    }
                }
            }

            let Some(pds_url) = endpoint else {
                metrics::BACKFILLER_REPOS_FAILED_TOTAL.inc();
                metrics::BACKFILLER_REPOS_RUNNING.dec();
                return Err(WintermuteError::Other(format!("no pds found: {did}")));
            };

            pds_cache.insert(did.to_owned(), pds_url.clone());
            pds_url
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
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();

        let mut batch_jobs: Vec<IndexJob> = Vec::with_capacity(leaves.len());

        for entry in &leaves {
            // entry.key is "collection/rkey" from MST -- split directly, skip regex
            let Some((collection, rkey)) = entry.key.split_once('/') else {
                continue;
            };

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
                let cid = entry.value.to_string();

                batch_jobs.push(IndexJob {
                    uri: uri_string,
                    cid,
                    action: WriteAction::Create,
                    record: Some(record_json),
                    indexed_at: now.clone(),
                    rev: rev.clone(),
                });
            }
        }

        if !batch_jobs.is_empty() {
            if job.priority {
                storage.enqueue_firehose_backfill_priority_batch(&batch_jobs)?;
            } else {
                storage.enqueue_firehose_backfill_batch(&batch_jobs)?;
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
