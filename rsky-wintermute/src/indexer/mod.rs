mod tests;

use crate::SHUTDOWN;
use crate::config::{INDEXER_BATCH_SIZE, WORKERS_INDEXER};
use crate::storage::Storage;
use crate::types::{IndexJob, WintermuteError, WriteAction};
use deadpool_postgres::{Config, ManagerConfig, Pool, RecyclingMethod, Runtime};
use rsky_syntax::aturi::AtUri;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio_postgres::NoTls;

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
enum QueueSource {
    FirehoseLive,
    FirehoseBackfill,
    LabelLive, // Future use for label stream processing
}

pub struct IndexerManager {
    workers: usize,
    storage: Arc<Storage>,
    pool: Pool,
    semaphore: Arc<Semaphore>,
}

impl IndexerManager {
    pub fn new(storage: Arc<Storage>, database_url: String) -> Result<Self, WintermuteError> {
        let mut pg_config = Config::new();
        pg_config.url = Some(database_url);
        pg_config.manager = Some(ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        });

        let pool = pg_config
            .create_pool(Some(Runtime::Tokio1), NoTls)
            .map_err(|e| WintermuteError::Other(format!("pool creation failed: {e}")))?;

        Ok(Self {
            workers: WORKERS_INDEXER,
            storage,
            pool,
            semaphore: Arc::new(Semaphore::new(WORKERS_INDEXER)),
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
        loop {
            if SHUTDOWN.load(Ordering::Relaxed) {
                tracing::info!("shutdown requested for indexer");
                break;
            }

            // Update queue length metrics for all three streams
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

            // Collect jobs from all three queues with priority: live > backfill > labels
            let mut jobs = Vec::new();

            // Priority 1: firehose_live (from ingester)
            for _ in 0..INDEXER_BATCH_SIZE {
                match self.storage.dequeue_firehose_live() {
                    Ok(Some((key, job))) => jobs.push((key, job, QueueSource::FirehoseLive)),
                    Ok(None) => break,
                    Err(e) => {
                        tracing::error!("failed to dequeue firehose_live job: {e}");
                        break;
                    }
                }
            }

            // Priority 2: firehose_backfill (from backfiller) if batch not full
            if jobs.len() < INDEXER_BATCH_SIZE {
                for _ in 0..(INDEXER_BATCH_SIZE - jobs.len()) {
                    match self.storage.dequeue_firehose_backfill() {
                        Ok(Some((key, job))) => {
                            jobs.push((key, job, QueueSource::FirehoseBackfill));
                        }
                        Ok(None) => break,
                        Err(e) => {
                            tracing::error!("failed to dequeue firehose_backfill job: {e}");
                            break;
                        }
                    }
                }
            }

            // Priority 3: label_live (future) if batch still not full
            // Labels will be processed separately, so we don't dequeue them here yet

            if jobs.is_empty() {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }

            let mut tasks = Vec::new();
            for (key, job, source) in jobs {
                let Ok(permit) = self.semaphore.clone().acquire_owned().await else {
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

    async fn process_job(pool: &Pool, job: &IndexJob) -> Result<(), WintermuteError> {
        use crate::metrics;

        metrics::INDEXER_RECORDS_PROCESSED_TOTAL.inc();

        let uri = AtUri::new(job.uri.clone(), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;

        let did = uri.get_hostname();
        let collection = uri.get_collection();
        let rkey = uri.get_rkey();

        let client = pool.get().await?;

        match job.action {
            WriteAction::Create | WriteAction::Update => {
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
        let text = record.get("text").and_then(|v| v.as_str()).unwrap_or("");
        let created_at = record
            .get("createdAt")
            .and_then(|v| v.as_str())
            .unwrap_or(indexed_at);

        client
            .execute(
                "INSERT INTO post (uri, cid, creator, text, \"createdAt\", \"indexedAt\")
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (uri) DO NOTHING",
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
                 ON CONFLICT (uri) DO NOTHING",
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

        let row_count = client
            .execute(
                "INSERT INTO follow (uri, cid, creator, \"subjectDid\", \"createdAt\", \"indexedAt\")
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (uri) DO NOTHING",
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
                 ON CONFLICT (uri) DO NOTHING",
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
        let display_name = record.get("displayName").and_then(|v| v.as_str());
        let description = record.get("description").and_then(|v| v.as_str());
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
                 ON CONFLICT (uri) DO NOTHING",
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
        let display_name = record.get("displayName").and_then(|v| v.as_str());
        let description = record.get("description").and_then(|v| v.as_str());
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
                 ON CONFLICT (uri) DO NOTHING",
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
        let name = record.get("name").and_then(|v| v.as_str());
        let purpose = record.get("purpose").and_then(|v| v.as_str());
        let description = record.get("description").and_then(|v| v.as_str());
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
                 ON CONFLICT (uri) DO NOTHING",
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

        client
            .execute(
                "INSERT INTO list_item (uri, cid, creator, \"listUri\", \"subjectDid\", \"createdAt\", \"indexedAt\")
                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT (uri) DO NOTHING",
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
                 ON CONFLICT (uri) DO NOTHING",
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
                 ON CONFLICT (uri) DO NOTHING",
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
        let policies = record.get("policies").map(std::string::ToString::to_string);
        let created_at = record
            .get("createdAt")
            .and_then(|v| v.as_str())
            .unwrap_or(indexed_at);

        client
            .execute(
                "INSERT INTO labeler (uri, cid, creator, policies, \"createdAt\", \"indexedAt\")
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (uri) DO NOTHING",
                &[&uri, &cid, &did, &policies, &created_at, &indexed_at],
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
                 ON CONFLICT (uri) DO NOTHING",
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
                 ON CONFLICT (uri) DO NOTHING",
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

    async fn index_notif_declaration(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
        record: &serde_json::Value,
        cid: &str,
        indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(
            format!("at://{did}/app.bsky.notification.declaration/{rkey}"),
            None,
        )
        .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        let allow_incoming = record.get("allowIncoming").and_then(|v| v.as_str());

        client
            .execute(
                "INSERT INTO notif_declaration (uri, cid, creator, \"allowIncoming\", \"indexedAt\")
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (uri) DO NOTHING",
                &[&uri, &cid, &did, &allow_incoming, &indexed_at],
            )
            .await?;

        Ok(())
    }

    async fn delete_notif_declaration(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(
            format!("at://{did}/app.bsky.notification.declaration/{rkey}"),
            None,
        )
        .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        client
            .execute("DELETE FROM notif_declaration WHERE uri = $1", &[&uri])
            .await?;
        Ok(())
    }

    async fn index_status(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
        record: &serde_json::Value,
        cid: &str,
        indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.actor.status/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        let status = record.get("status").and_then(|v| v.as_str());

        client
            .execute(
                "INSERT INTO status (uri, cid, creator, status, \"indexedAt\")
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (uri) DO NOTHING",
                &[&uri, &cid, &did, &status, &indexed_at],
            )
            .await?;

        Ok(())
    }

    async fn delete_status(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
    ) -> Result<(), WintermuteError> {
        let uri_obj = AtUri::new(format!("at://{did}/app.bsky.actor.status/{rkey}"), None)
            .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))?;
        let uri = uri_obj.to_string();
        client
            .execute("DELETE FROM status WHERE uri = $1", &[&uri])
            .await?;
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
        let proof = record.get("proof").and_then(|v| v.as_str());

        client
            .execute(
                "INSERT INTO verification (uri, cid, creator, proof, \"indexedAt\")
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (uri) DO NOTHING",
                &[&uri, &cid, &did, &proof, &indexed_at],
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
