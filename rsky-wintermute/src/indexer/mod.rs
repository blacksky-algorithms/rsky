mod tests;

use crate::SHUTDOWN;
use crate::config::{INDEXER_BATCH_SIZE, WORKERS_INDEXER};
use crate::storage::Storage;
use crate::types::{IndexJob, WintermuteError, WriteAction};
use deadpool_postgres::{Config, ManagerConfig, Pool, RecyclingMethod, Runtime};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio_postgres::NoTls;

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

            let mut jobs = Vec::new();
            for _ in 0..INDEXER_BATCH_SIZE {
                match self.storage.dequeue_index() {
                    Ok(Some((key, job))) => jobs.push((key, job)),
                    Ok(None) => break,
                    Err(e) => {
                        tracing::error!("failed to dequeue index job: {e}");
                        break;
                    }
                }
            }

            if jobs.is_empty() {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }

            let mut tasks = Vec::new();
            for (key, job) in jobs {
                let Ok(permit) = self.semaphore.clone().acquire_owned().await else {
                    break;
                };

                let pool = self.pool.clone();

                let task = tokio::spawn(async move {
                    let result = Self::process_job(&pool, &job).await;
                    drop(permit);
                    (key, result)
                });

                tasks.push(task);
            }

            for task in tasks {
                match task.await {
                    Ok((key, Ok(()))) => {
                        if let Err(e) = self.storage.remove_index(&key) {
                            tracing::error!("failed to remove index job: {e}");
                        }
                    }
                    Ok((_, Err(e))) => {
                        tracing::error!("index job failed: {e}");
                    }
                    Err(e) => {
                        tracing::error!("task panicked: {e}");
                    }
                }
            }
        }
    }

    async fn process_job(pool: &Pool, job: &IndexJob) -> Result<(), WintermuteError> {
        let uri_parts: Vec<&str> = job
            .uri
            .strip_prefix("at://")
            .unwrap_or(&job.uri)
            .split('/')
            .collect();
        if uri_parts.len() != 3 {
            return Err(WintermuteError::Other(format!("invalid uri: {}", job.uri)));
        }

        let did = uri_parts[0];
        let collection = uri_parts[1];
        let rkey = uri_parts[2];

        let client = pool.get().await?;

        match job.action {
            WriteAction::Create | WriteAction::Update => {
                let record_json = job.record.as_ref().ok_or_else(|| {
                    WintermuteError::Other("missing record for create/update".into())
                })?;

                match collection {
                    "app.bsky.feed.post" => {
                        Self::index_post(&client, did, rkey, record_json, &job.indexed_at).await?;
                    }
                    "app.bsky.feed.like" => {
                        Self::index_like(&client, did, rkey, record_json, &job.indexed_at).await?;
                    }
                    "app.bsky.graph.follow" => {
                        Self::index_follow(&client, did, rkey, record_json, &job.indexed_at)
                            .await?;
                    }
                    "app.bsky.feed.repost" => {
                        Self::index_repost(&client, did, rkey, record_json, &job.indexed_at)
                            .await?;
                    }
                    _ => {}
                }
            }
            WriteAction::Delete => match collection {
                "app.bsky.feed.post" => {
                    Self::delete_post(&client, did, rkey).await?;
                }
                "app.bsky.feed.like" => {
                    Self::delete_like(&client, did, rkey).await?;
                }
                "app.bsky.graph.follow" => {
                    Self::delete_follow(&client, did, rkey).await?;
                }
                "app.bsky.feed.repost" => {
                    Self::delete_repost(&client, did, rkey).await?;
                }
                _ => {}
            },
        }

        Ok(())
    }

    async fn index_post(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
        record: &serde_json::Value,
        indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        let uri = format!("at://{did}/app.bsky.feed.post/{rkey}");
        let text = record.get("text").and_then(|v| v.as_str()).unwrap_or("");
        let created_at = record
            .get("createdAt")
            .and_then(|v| v.as_str())
            .unwrap_or(indexed_at);

        client
            .execute(
                "INSERT INTO post (uri, creator, text, created_at, indexed_at)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (uri) DO UPDATE SET text = $3, indexed_at = $5",
                &[&uri, &did, &text, &created_at, &indexed_at],
            )
            .await?;

        Ok(())
    }

    async fn delete_post(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
    ) -> Result<(), WintermuteError> {
        let uri = format!("at://{did}/app.bsky.feed.post/{rkey}");
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
        indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        let uri = format!("at://{did}/app.bsky.feed.like/{rkey}");
        let subject = record
            .get("subject")
            .and_then(|v| v.get("uri"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let created_at = record
            .get("createdAt")
            .and_then(|v| v.as_str())
            .unwrap_or(indexed_at);

        client
            .execute(
                "INSERT INTO like (uri, creator, subject, created_at, indexed_at)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (uri) DO UPDATE SET indexed_at = $5",
                &[&uri, &did, &subject, &created_at, &indexed_at],
            )
            .await?;

        Ok(())
    }

    async fn delete_like(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
    ) -> Result<(), WintermuteError> {
        let uri = format!("at://{did}/app.bsky.feed.like/{rkey}");
        client
            .execute("DELETE FROM like WHERE uri = $1", &[&uri])
            .await?;
        Ok(())
    }

    async fn index_follow(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
        record: &serde_json::Value,
        indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        let uri = format!("at://{did}/app.bsky.graph.follow/{rkey}");
        let subject = record.get("subject").and_then(|v| v.as_str()).unwrap_or("");
        let created_at = record
            .get("createdAt")
            .and_then(|v| v.as_str())
            .unwrap_or(indexed_at);

        client
            .execute(
                "INSERT INTO follow (uri, creator, subject, created_at, indexed_at)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (uri) DO UPDATE SET indexed_at = $5",
                &[&uri, &did, &subject, &created_at, &indexed_at],
            )
            .await?;

        Ok(())
    }

    async fn delete_follow(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
    ) -> Result<(), WintermuteError> {
        let uri = format!("at://{did}/app.bsky.graph.follow/{rkey}");
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
        indexed_at: &str,
    ) -> Result<(), WintermuteError> {
        let uri = format!("at://{did}/app.bsky.feed.repost/{rkey}");
        let subject = record
            .get("subject")
            .and_then(|v| v.get("uri"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let created_at = record
            .get("createdAt")
            .and_then(|v| v.as_str())
            .unwrap_or(indexed_at);

        client
            .execute(
                "INSERT INTO repost (uri, creator, subject, created_at, indexed_at)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (uri) DO UPDATE SET indexed_at = $5",
                &[&uri, &did, &subject, &created_at, &indexed_at],
            )
            .await?;

        Ok(())
    }

    async fn delete_repost(
        client: &deadpool_postgres::Client,
        did: &str,
        rkey: &str,
    ) -> Result<(), WintermuteError> {
        let uri = format!("at://{did}/app.bsky.feed.repost/{rkey}");
        client
            .execute("DELETE FROM repost WHERE uri = $1", &[&uri])
            .await?;
        Ok(())
    }
}
