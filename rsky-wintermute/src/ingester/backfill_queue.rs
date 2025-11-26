use crate::SHUTDOWN;
use crate::storage::Storage;
use crate::types::{BackfillJob, WintermuteError};
use deadpool_postgres::{Config, ManagerConfig, Pool, RecyclingMethod, Runtime};
use serde::Deserialize;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio_postgres::NoTls;

#[derive(Debug, Deserialize)]
struct ListReposResponse {
    repos: Vec<RepoRef>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RepoRef {
    did: String,
}

/// Cursor storage abstraction for testing
enum CursorStore<'a> {
    Postgres(&'a Pool),
    Fjall(&'a Storage),
}

impl CursorStore<'_> {
    async fn get(&self, key: &str) -> Result<Option<i64>, WintermuteError> {
        match self {
            CursorStore::Postgres(pool) => get_cursor_from_postgres(pool, key).await,
            CursorStore::Fjall(storage) => Ok(storage.get_cursor(key)?),
        }
    }

    async fn set(&self, key: &str, cursor: i64) -> Result<(), WintermuteError> {
        match self {
            CursorStore::Postgres(pool) => set_cursor_in_postgres(pool, key, cursor).await,
            CursorStore::Fjall(storage) => storage.set_cursor(key, cursor),
        }
    }

    async fn delete(&self, key: &str) -> Result<(), WintermuteError> {
        match self {
            CursorStore::Postgres(pool) => delete_cursor_from_postgres(pool, key).await,
            CursorStore::Fjall(storage) => storage.delete_cursor(key),
        }
    }
}

pub async fn populate_backfill_queue(
    storage: Arc<Storage>,
    relay_host: String,
    database_url: String,
) -> Result<(), WintermuteError> {
    use crate::metrics;

    // Create postgres pool for cursor storage (if database_url provided)
    let pool = if database_url.is_empty() {
        None
    } else {
        let mut pg_config = Config::new();
        pg_config.url = Some(database_url);
        pg_config.manager = Some(ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        });

        Some(
            pg_config
                .create_pool(Some(Runtime::Tokio1), NoTls)
                .map_err(|e| {
                    WintermuteError::Other(format!("backfill pool creation failed: {e}"))
                })?,
        )
    };

    let cursor_store = pool
        .as_ref()
        .map_or_else(|| CursorStore::Fjall(&storage), CursorStore::Postgres);

    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    let cursor_key = format!("backfill_enum:{relay_host}");

    // Get cursor from cursor store (postgres survives Fjall corruption)
    let stored_cursor = cursor_store.get(&cursor_key).await?;

    // Check if Fjall was lost: cursor exists but repo_backfill queue is empty
    // This means DIDs were enumerated but the queue data was lost
    let repo_backfill_len = storage.repo_backfill_len().unwrap_or(0);
    let mut cursor = if let Some(stored) = stored_cursor {
        if stored > 0 && repo_backfill_len == 0 {
            tracing::warn!(
                "detected Fjall data loss: cursor at {} but repo_backfill queue is empty, resetting to re-enumerate from beginning",
                stored
            );
            metrics::INGESTER_BACKFILL_CURSOR_RESET_TOTAL.inc();
            // Reset cursor
            cursor_store.delete(&cursor_key).await?;
            None
        } else {
            tracing::info!("resuming backfill enumeration from cursor {}", stored);
            Some(stored.to_string())
        }
    } else {
        None
    };

    // Preserve the scheme (http:// or https://) from the original URL for testing
    let (scheme, clean_hostname) = if relay_host.starts_with("http://") {
        (
            "http",
            relay_host
                .trim_start_matches("http://")
                .trim_end_matches('/'),
        )
    } else {
        (
            "https",
            relay_host
                .trim_start_matches("https://")
                .trim_end_matches('/'),
        )
    };

    let mut total_enumerated = 0u64;
    let mut last_log_count = 0u64;

    loop {
        if SHUTDOWN.load(Ordering::Relaxed) {
            tracing::info!("shutdown requested for backfill enumeration");
            break;
        }

        let mut url = url::Url::parse(&format!(
            "{scheme}://{clean_hostname}/xrpc/com.atproto.sync.listRepos"
        ))
        .map_err(|e| WintermuteError::Other(format!("invalid url: {e}")))?;

        url.query_pairs_mut().append_pair("limit", "1000");
        if let Some(ref c) = cursor {
            url.query_pairs_mut().append_pair("cursor", c);
        }

        let response = match http_client.get(url.as_str()).send().await {
            Ok(r) => r,
            Err(e) => {
                metrics::INGESTER_BACKFILL_FETCH_ERRORS_TOTAL.inc();
                return Err(WintermuteError::Other(format!("http error: {e}")));
            }
        };

        if !response.status().is_success() {
            metrics::INGESTER_BACKFILL_FETCH_ERRORS_TOTAL.inc();
            return Err(WintermuteError::Other(format!(
                "http error: {}",
                response.status()
            )));
        }

        let list_response: ListReposResponse = response.json().await?;

        for repo in &list_response.repos {
            metrics::INGESTER_BACKFILL_REPOS_FETCHED_TOTAL.inc();
            let job = BackfillJob {
                did: repo.did.clone(),
                retry_count: 0,
            };
            storage.enqueue_backfill(&job)?;
            metrics::INGESTER_BACKFILL_REPOS_WRITTEN_TOTAL.inc();
            total_enumerated += 1;
        }

        if let Some(next_cursor) = list_response.cursor {
            // Store cursor (survives Fjall corruption when using postgres)
            cursor_store
                .set(&cursor_key, next_cursor.parse::<i64>().unwrap_or(0))
                .await?;
            cursor = Some(next_cursor.clone());

            // Log every 10K repos
            if total_enumerated / 10_000 > last_log_count {
                last_log_count = total_enumerated / 10_000;
                let queue_len = storage.repo_backfill_len().unwrap_or(0);
                tracing::info!(
                    "enumerated {} repos, cursor={}, queue_len={}",
                    total_enumerated,
                    next_cursor,
                    queue_len
                );
            }
        } else {
            let queue_len = storage.repo_backfill_len().unwrap_or(0);
            tracing::info!(
                "backfill enumeration complete: {} repos, queue_len={}",
                total_enumerated,
                queue_len
            );
            metrics::INGESTER_BACKFILL_COMPLETE.set(1);
            break;
        }
    }

    Ok(())
}

async fn get_cursor_from_postgres(
    pool: &Pool,
    service: &str,
) -> Result<Option<i64>, WintermuteError> {
    let client = pool.get().await?;
    let row = client
        .query_opt(
            "SELECT cursor FROM sub_state WHERE service = $1",
            &[&service],
        )
        .await?;

    Ok(row.map(|r| r.get::<_, i64>("cursor")))
}

async fn set_cursor_in_postgres(
    pool: &Pool,
    service: &str,
    cursor: i64,
) -> Result<(), WintermuteError> {
    let client = pool.get().await?;
    client
        .execute(
            "INSERT INTO sub_state (service, cursor)
             VALUES ($1, $2)
             ON CONFLICT (service) DO UPDATE SET cursor = EXCLUDED.cursor",
            &[&service, &cursor],
        )
        .await?;
    Ok(())
}

async fn delete_cursor_from_postgres(pool: &Pool, service: &str) -> Result<(), WintermuteError> {
    let client = pool.get().await?;
    client
        .execute("DELETE FROM sub_state WHERE service = $1", &[&service])
        .await?;
    Ok(())
}
