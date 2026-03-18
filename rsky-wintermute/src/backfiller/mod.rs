mod tests;

use crate::SHUTDOWN;
use crate::config::{
    BACKFILLER_BATCH_SIZE, BACKFILLER_DB_POOL_SIZE, BACKFILLER_DIRECT_WRITE, WORKERS_BACKFILLER,
    backfiller_timeout,
};
use crate::storage::Storage;
use crate::types::{BackfillJob, IndexJob, WintermuteError, WriteAction};
use dashmap::DashMap;
use deadpool_postgres::{Config, ManagerConfig, Pool, RecyclingMethod, Runtime};
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
use tokio_postgres::NoTls;

pub struct BackfillerManager {
    workers: usize,
    storage: Arc<Storage>,
    http_client: reqwest::Client,
    pds_cache: Arc<DashMap<String, String>>,
    pool: Option<Pool>,
}

impl BackfillerManager {
    pub fn new(storage: Arc<Storage>, database_url: &str) -> Result<Self, WintermuteError> {
        let workers = *WORKERS_BACKFILLER;
        let http_client = reqwest::Client::builder()
            .timeout(backfiller_timeout())
            .build()?;

        let pool = if *BACKFILLER_DIRECT_WRITE {
            let pool_size = *BACKFILLER_DB_POOL_SIZE;
            tracing::info!(
                "backfiller direct write enabled, PG pool size={}",
                pool_size
            );
            let mut pg_config = Config::new();
            pg_config.url = Some(database_url.to_owned());
            pg_config.manager = Some(ManagerConfig {
                recycling_method: RecyclingMethod::Fast,
            });
            pg_config.pool = Some(deadpool_postgres::PoolConfig::new(pool_size));
            Some(
                pg_config
                    .create_pool(Some(Runtime::Tokio1), NoTls)
                    .map_err(|e| {
                        WintermuteError::Other(format!("backfiller pool creation failed: {e}"))
                    })?,
            )
        } else {
            tracing::info!("backfiller direct write disabled, using Fjall queue");
            None
        };

        tracing::info!(
            "backfiller config: workers={}, batch_size={}, timeout={:?}, direct_write={}",
            workers,
            *BACKFILLER_BATCH_SIZE,
            backfiller_timeout(),
            pool.is_some()
        );

        Ok(Self {
            workers,
            storage,
            http_client,
            pds_cache: Arc::new(DashMap::new()),
            pool,
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
            let pool = self.pool.clone();

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

                    match Self::process_job(&storage, &http_client, &pds_cache, &pool, &job).await {
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
        pool: &Option<Pool>,
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

        let repo = ReadableRepo::load(storage_arc, root)
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

        // Use reachable_leaves() instead of list() to handle repos with missing MST blocks.
        // list() uses NodeIter which silently drops entire subtrees via unwrap_or_default().
        // reachable_leaves() uses NodeIterReachable which skips missing blocks with logging.
        let repo_storage = repo.storage.clone();
        let rev = repo.commit.rev.clone();
        let leaves = repo
            .data
            .reachable_leaves()
            .await
            .map_err(|e| WintermuteError::Repo(format!("reachable_leaves failed: {e}")))?;

        let blocks_result = {
            let storage_guard = repo_storage.read().await;
            storage_guard
                .get_blocks(leaves.iter().map(|e| e.value).collect())
                .await
                .map_err(|e| WintermuteError::Repo(format!("get blocks failed: {e}")))?
        };
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
            if let Some(pool) = pool {
                Self::direct_write_to_pg(pool, did, &batch_jobs, &now).await?;
            } else if job.priority {
                storage.enqueue_firehose_backfill_priority_batch(&batch_jobs)?;
            } else {
                storage.enqueue_firehose_backfill_batch(&batch_jobs)?;
            }
        }

        metrics::BACKFILLER_REPOS_PROCESSED_TOTAL.inc();
        metrics::BACKFILLER_REPOS_RUNNING.dec();

        Ok(())
    }

    /// Write extracted records directly to `PostgreSQL`, bypassing the Fjall/LMDB queue.
    /// Routes records by collection type and calls the appropriate bulk COPY functions.
    async fn direct_write_to_pg(
        pool: &Pool,
        did: &str,
        jobs: &[IndexJob],
        indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        use crate::indexer::IndexerManager;
        use crate::indexer::bulk;
        use crate::indexer::sanitize_opt;
        use crate::indexer::sanitize_text;
        use crate::metrics;
        use std::time::Instant;

        let batch_start = Instant::now();
        let client = pool
            .get()
            .await
            .map_err(|e| WintermuteError::Other(format!("backfiller pool error: {e}")))?;

        // 1. Ensure actor exists
        bulk::copy_ensure_actors(&client, &[did]).await?;

        // 2. Insert all records into the record table with stale detection
        let record_data: Vec<(String, String, String, String, String, String)> = jobs
            .iter()
            .map(|j| {
                (
                    j.uri.clone(),
                    j.cid.clone(),
                    did.to_owned(),
                    j.record
                        .as_ref()
                        .map(|r| serde_json::to_string(r).unwrap_or_default())
                        .unwrap_or_default(),
                    j.rev.clone(),
                    j.indexed_at.clone(),
                )
            })
            .collect();

        let applied = bulk::copy_insert_records(&client, &record_data).await?;

        // 3. Route applied (non-stale) records by collection
        let mut post_data: Vec<(String, String, String, String, String, String)> = Vec::new();
        let mut feed_item_data: Vec<(String, String, String, String, String, String)> = Vec::new();
        let mut like_data: Vec<(String, String, String, String, String, String, String)> =
            Vec::new();
        let mut follow_data: Vec<(String, String, String, String, String, String)> = Vec::new();
        let mut repost_data: Vec<(String, String, String, String, String, String, String)> =
            Vec::new();
        let mut repost_feed_items: Vec<(String, String, String, String, String, String)> =
            Vec::new();
        let mut block_data: Vec<(String, String, String, String, String, String)> = Vec::new();
        let mut profile_uris: Vec<String> = Vec::new();
        let mut profile_cids: Vec<String> = Vec::new();
        let mut profile_creators: Vec<String> = Vec::new();
        let mut profile_display_names: Vec<Option<String>> = Vec::new();
        let mut profile_descriptions: Vec<Option<String>> = Vec::new();
        let mut profile_avatar_cids: Vec<Option<String>> = Vec::new();
        let mut profile_banner_cids: Vec<Option<String>> = Vec::new();
        let mut profile_indexed_ats: Vec<String> = Vec::new();
        let mut embed_image_data: Vec<(String, String, String, String)> = Vec::new();
        let mut embed_video_data: Vec<(String, String, Option<String>)> = Vec::new();
        let mut other_count: u64 = 0;
        let mut stale_count: u64 = 0;

        for (i, job) in jobs.iter().enumerate() {
            if !applied[i] {
                stale_count += 1;
                continue;
            }

            let Some(record) = &job.record else {
                continue;
            };

            // Extract collection from URI: at://did/collection/rkey
            let collection = job
                .uri
                .strip_prefix("at://")
                .and_then(|rest| rest.split('/').nth(1))
                .unwrap_or("");

            match collection {
                "app.bsky.feed.post" => {
                    let text =
                        sanitize_text(record.get("text").and_then(|v| v.as_str()).unwrap_or(""));
                    let created_at = record
                        .get("createdAt")
                        .and_then(|v| v.as_str())
                        .unwrap_or(indexed_at)
                        .to_owned();
                    let sort_at = if indexed_at < created_at.as_str() {
                        indexed_at.to_owned()
                    } else {
                        created_at.clone()
                    };

                    post_data.push((
                        job.uri.clone(),
                        job.cid.clone(),
                        did.to_owned(),
                        text,
                        created_at,
                        indexed_at.to_owned(),
                    ));

                    feed_item_data.push((
                        "post".to_owned(),
                        job.uri.clone(),
                        job.cid.clone(),
                        job.uri.clone(),
                        did.to_owned(),
                        sort_at,
                    ));

                    // Extract embed images/videos
                    if let Some(embed) = record.get("embed") {
                        IndexerManager::extract_embed_data(
                            embed,
                            &job.uri,
                            &mut embed_image_data,
                            &mut embed_video_data,
                        );
                    }
                }
                "app.bsky.feed.like" => {
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
                        .unwrap_or(indexed_at);

                    like_data.push((
                        job.uri.clone(),
                        job.cid.clone(),
                        did.to_owned(),
                        subject_uri.to_owned(),
                        subject_cid.to_owned(),
                        created_at.to_owned(),
                        indexed_at.to_owned(),
                    ));
                }
                "app.bsky.graph.follow" => {
                    let subject = record.get("subject").and_then(|v| v.as_str()).unwrap_or("");
                    let created_at = record
                        .get("createdAt")
                        .and_then(|v| v.as_str())
                        .unwrap_or(indexed_at);

                    follow_data.push((
                        job.uri.clone(),
                        job.cid.clone(),
                        did.to_owned(),
                        subject.to_owned(),
                        created_at.to_owned(),
                        indexed_at.to_owned(),
                    ));
                }
                "app.bsky.feed.repost" => {
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
                        .unwrap_or(indexed_at)
                        .to_owned();
                    let sort_at = if indexed_at < created_at.as_str() {
                        indexed_at.to_owned()
                    } else {
                        created_at.clone()
                    };

                    repost_data.push((
                        job.uri.clone(),
                        job.cid.clone(),
                        did.to_owned(),
                        subject_uri.to_owned(),
                        subject_cid.to_owned(),
                        created_at,
                        indexed_at.to_owned(),
                    ));

                    repost_feed_items.push((
                        "repost".to_owned(),
                        job.uri.clone(),
                        job.cid.clone(),
                        subject_uri.to_owned(),
                        did.to_owned(),
                        sort_at,
                    ));
                }
                "app.bsky.graph.block" => {
                    let subject = record.get("subject").and_then(|v| v.as_str()).unwrap_or("");
                    let created_at = record
                        .get("createdAt")
                        .and_then(|v| v.as_str())
                        .unwrap_or(indexed_at);

                    block_data.push((
                        job.uri.clone(),
                        job.cid.clone(),
                        did.to_owned(),
                        subject.to_owned(),
                        created_at.to_owned(),
                        indexed_at.to_owned(),
                    ));
                }
                "app.bsky.actor.profile" => {
                    let display_name =
                        sanitize_opt(record.get("displayName").and_then(|v| v.as_str()));
                    let description =
                        sanitize_opt(record.get("description").and_then(|v| v.as_str()));
                    let avatar_cid = record
                        .get("avatar")
                        .and_then(|v| v.get("ref"))
                        .and_then(|v| v.get("$link"))
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    let banner_cid = record
                        .get("banner")
                        .and_then(|v| v.get("ref"))
                        .and_then(|v| v.get("$link"))
                        .and_then(|v| v.as_str())
                        .map(String::from);

                    profile_uris.push(job.uri.clone());
                    profile_cids.push(job.cid.clone());
                    profile_creators.push(did.to_owned());
                    profile_display_names.push(display_name);
                    profile_descriptions.push(description);
                    profile_avatar_cids.push(avatar_cid);
                    profile_banner_cids.push(banner_cid);
                    profile_indexed_ats.push(indexed_at.to_owned());
                }
                other_collection => {
                    let rkey = job
                        .uri
                        .strip_prefix("at://")
                        .and_then(|rest| rest.split('/').nth(2))
                        .unwrap_or("");
                    if let Err(e) = IndexerManager::process_collection_specific(
                        &client,
                        other_collection,
                        did,
                        rkey,
                        record,
                        &job.cid,
                        indexed_at,
                    )
                    .await
                    {
                        tracing::warn!(
                            "direct write: {} failed for {}: {e}",
                            other_collection,
                            job.uri
                        );
                    }
                    other_count += 1;
                }
            }
        }

        // 4. Call bulk COPY functions for each collection type
        bulk::copy_insert_posts_core(&client, &post_data).await?;
        bulk::copy_insert_feed_items(&client, &feed_item_data).await?;
        bulk::copy_insert_likes(&client, &like_data).await?;
        bulk::copy_insert_follows_core(&client, &follow_data).await?;
        bulk::copy_insert_reposts(&client, &repost_data).await?;
        bulk::copy_insert_feed_items(&client, &repost_feed_items).await?;
        bulk::copy_insert_blocks(&client, &block_data).await?;
        bulk::copy_insert_post_embed_images(&client, &embed_image_data).await?;
        bulk::copy_insert_post_embed_videos(&client, &embed_video_data).await?;

        // Insert profiles via unnest (same pattern as batch_insert_profiles)
        if !profile_uris.is_empty() {
            client
                .execute(
                    "INSERT INTO profile (uri, cid, creator, \"displayName\", description, \"avatarCid\", \"bannerCid\", \"indexedAt\")
                     SELECT * FROM unnest($1::text[], $2::text[], $3::text[], $4::text[], $5::text[], $6::text[], $7::text[], $8::text[])
                     ON CONFLICT (uri) DO UPDATE SET
                       cid = EXCLUDED.cid,
                       \"displayName\" = EXCLUDED.\"displayName\",
                       description = EXCLUDED.description,
                       \"avatarCid\" = EXCLUDED.\"avatarCid\",
                       \"bannerCid\" = EXCLUDED.\"bannerCid\",
                       \"indexedAt\" = EXCLUDED.\"indexedAt\"",
                    &[
                        &profile_uris,
                        &profile_cids,
                        &profile_creators,
                        &profile_display_names,
                        &profile_descriptions,
                        &profile_avatar_cids,
                        &profile_banner_cids,
                        &profile_indexed_ats,
                    ],
                )
                .await?;
        }

        // 5. Corrective profile_agg COUNT for the creator DID only
        //    Skip subject followersCount -- corrected when their repos are backfilled.
        client
            .execute(
                "INSERT INTO profile_agg (did, \"followsCount\", \"postsCount\")
                 SELECT $1::varchar,
                        (SELECT COUNT(*) FROM follow WHERE creator = $1),
                        (SELECT COUNT(*) FROM post WHERE creator = $1)
                 ON CONFLICT (did) DO UPDATE SET
                   \"followsCount\" = EXCLUDED.\"followsCount\",
                   \"postsCount\" = EXCLUDED.\"postsCount\"",
                &[&did],
            )
            .await?;

        let total_ms = batch_start.elapsed().as_millis();
        let applied_count = applied.iter().filter(|a| **a).count();
        metrics::BACKFILLER_DIRECT_WRITE_TOTAL.inc();
        metrics::BACKFILLER_DIRECT_WRITE_RECORDS_TOTAL.inc_by(applied_count as u64);
        metrics::BACKFILLER_DIRECT_WRITE_STALE_TOTAL.inc_by(stale_count);

        tracing::info!(
            "direct write {}: {}ms, records={}, applied={}, stale={}, posts={}, likes={}, follows={}, reposts={}, blocks={}, profiles={}, images={}, videos={}, other={}",
            did,
            total_ms,
            jobs.len(),
            applied_count,
            stale_count,
            post_data.len(),
            like_data.len(),
            follow_data.len(),
            repost_data.len(),
            block_data.len(),
            profile_uris.len(),
            embed_image_data.len(),
            embed_video_data.len(),
            other_count,
        );

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
