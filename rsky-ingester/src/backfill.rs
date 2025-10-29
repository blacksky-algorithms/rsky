use crate::batcher::Batcher;
use crate::{BackfillEvent, IngesterConfig, IngesterError, streams};
use redis::AsyncCommands;
use serde::Deserialize;
use tokio::time::Duration;
use tracing::{error, info, warn};

const INGESTER_DONE_CURSOR: &str = "!ingester-done";

/// Response from com.atproto.sync.listRepos
#[derive(Debug, Deserialize)]
struct ListReposResponse {
    repos: Vec<RepoRef>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RepoRef {
    did: String,
    #[allow(dead_code)]
    head: String,
    rev: String,
    active: Option<bool>,
    status: Option<String>,
}

/// BackfillIngester calls com.atproto.sync.listRepos with pagination
/// and writes repo backfill events to the repo_backfill Redis stream
pub struct BackfillIngester {
    config: IngesterConfig,
    redis_client: redis::Client,
    http_client: reqwest::Client,
}

impl BackfillIngester {
    pub fn new(config: IngesterConfig) -> Result<Self, IngesterError> {
        let redis_client = redis::Client::open(config.redis_url.clone())?;
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        Ok(Self {
            config,
            redis_client,
            http_client,
        })
    }

    pub async fn run(&self, hostname: String) -> Result<(), IngesterError> {
        info!("Starting BackfillIngester for {}", hostname);

        loop {
            match self.run_backfill(&hostname).await {
                Ok(()) => {
                    info!("Backfill complete for {}", hostname);
                    // Wait before checking again
                    tokio::time::sleep(Duration::from_secs(300)).await;
                }
                Err(e) => {
                    error!("BackfillIngester error for {}: {:?}", hostname, e);
                    tokio::time::sleep(Duration::from_secs(30)).await;
                }
            }
        }
    }

    async fn run_backfill(&self, hostname: &str) -> Result<(), IngesterError> {
        let mut conn = self.redis_client.get_multiplexed_async_connection().await?;

        // Get cursor from Redis
        let cursor_key = format!("{}:cursor:{}", streams::REPO_BACKFILL, hostname);
        let cursor: Option<String> = conn.get(&cursor_key).await.unwrap_or(None);

        // Check if backfill is already complete
        if cursor.as_deref() == Some(INGESTER_DONE_CURSOR) {
            info!("Backfill already complete for {}", hostname);
            return Ok(());
        }

        info!("Starting backfill from cursor {:?}", cursor);

        // Create batcher for events
        let (batch_tx, mut batch_rx) = Batcher::new(
            self.config.batch_size,
            self.config.batch_timeout_ms,
        );

        // Spawn task to handle batched writes to Redis
        let redis_client = self.redis_client.clone();
        let high_water_mark = self.config.high_water_mark;
        let write_task = tokio::spawn(async move {
            let mut conn = redis_client.get_multiplexed_async_connection().await?;

            while let Some(batch) = batch_rx.recv().await {
                // Check backpressure
                let stream_len: usize = conn.xlen(streams::REPO_BACKFILL).await?;
                if stream_len >= high_water_mark {
                    warn!(
                        "Backpressure: stream length {} >= {}",
                        stream_len, high_water_mark
                    );
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }

                // Write batch to Redis stream
                Self::write_batch(&mut conn, &batch).await?;
            }

            Ok::<_, IngesterError>(())
        });

        let mut current_cursor = cursor;
        let mut total_repos = 0;

        // Paginate through listRepos
        loop {
            let repos = self.fetch_repos(hostname, current_cursor.as_deref()).await?;

            for repo in &repos.repos {
                let event = BackfillEvent {
                    did: repo.did.clone(),
                    host: format!("https://{}", hostname),
                    rev: repo.rev.clone(),
                    status: repo.status.clone(),
                    active: repo.active.unwrap_or(true),
                };

                if let Err(e) = batch_tx.send(event) {
                    error!("Failed to send event to batcher: {:?}", e);
                    break;
                }

                total_repos += 1;
            }

            // Update cursor in Redis
            if let Some(ref next_cursor) = repos.cursor {
                conn.set::<_, _, ()>(&cursor_key, next_cursor).await?;
                current_cursor = Some(next_cursor.clone());

                if total_repos % 10000 == 0 {
                    info!("Processed {} repos, cursor: {}", total_repos, next_cursor);
                }
            } else {
                // No more repos, mark as done
                conn.set::<_, _, ()>(&cursor_key, INGESTER_DONE_CURSOR).await?;
                info!("Backfill complete! Total repos: {}", total_repos);
                break;
            }
        }

        // Cleanup
        drop(batch_tx);
        write_task.await.map_err(|e| IngesterError::Other(e.into()))??;

        Ok(())
    }

    async fn fetch_repos(
        &self,
        hostname: &str,
        cursor: Option<&str>,
    ) -> Result<ListReposResponse, IngesterError> {
        let mut url = url::Url::parse(&format!(
            "https://{}/xrpc/com.atproto.sync.listRepos",
            hostname
        ))
        .map_err(|e| IngesterError::Other(e.into()))?;

        url.query_pairs_mut().append_pair("limit", "1000");

        if let Some(cursor) = cursor {
            url.query_pairs_mut().append_pair("cursor", cursor);
        }

        let response = self.http_client.get(url).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(IngesterError::Other(anyhow::anyhow!(
                "listRepos failed: {} - {}",
                status,
                body
            )));
        }

        let repos = response.json::<ListReposResponse>().await?;
        Ok(repos)
    }

    async fn write_batch(
        conn: &mut redis::aio::MultiplexedConnection,
        batch: &[BackfillEvent],
    ) -> Result<(), IngesterError> {
        let mut pipe = redis::pipe();
        pipe.atomic();

        for event in batch {
            let event_json = serde_json::to_string(event)
                .map_err(|e| IngesterError::Serialization(e.to_string()))?;

            pipe.xadd(streams::REPO_BACKFILL, "*", &[("repo", event_json)]);
        }

        pipe.query_async::<()>(conn).await?;

        Ok(())
    }
}
