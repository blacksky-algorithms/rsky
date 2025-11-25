use crate::storage::Storage;
use crate::types::{BackfillJob, WintermuteError};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Deserialize)]
struct ListReposResponse {
    repos: Vec<RepoRef>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RepoRef {
    did: String,
}

pub async fn populate_backfill_queue(
    storage: Arc<Storage>,
    relay_host: String,
) -> Result<(), WintermuteError> {
    use crate::metrics;

    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    let cursor_key = format!("backfill_enum:{relay_host}");
    let mut cursor = storage.get_cursor(&cursor_key)?.map(|c| c.to_string());

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

    loop {
        let mut url = url::Url::parse(&format!(
            "{scheme}://{clean_hostname}/xrpc/com.atproto.sync.listRepos"
        ))
        .map_err(|e| WintermuteError::Other(format!("invalid url: {e}")))?;

        url.query_pairs_mut().append_pair("limit", "1000");
        if let Some(ref c) = cursor {
            url.query_pairs_mut().append_pair("cursor", c);
        }

        tracing::info!("fetching repos from {url}");

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

        tracing::info!("received {} repos", list_response.repos.len());

        for repo in &list_response.repos {
            metrics::INGESTER_BACKFILL_REPOS_FETCHED_TOTAL.inc();
            let job = BackfillJob {
                did: repo.did.clone(),
                retry_count: 0,
            };
            storage.enqueue_backfill(&job)?;
            metrics::INGESTER_BACKFILL_REPOS_WRITTEN_TOTAL.inc();
        }

        if let Some(next_cursor) = list_response.cursor {
            storage.set_cursor(&cursor_key, next_cursor.parse::<i64>().unwrap_or(0))?;
            cursor = Some(next_cursor);
        } else {
            tracing::info!("backfill queue population complete for {relay_host}");
            metrics::INGESTER_BACKFILL_COMPLETE.set(1);
            break;
        }
    }

    Ok(())
}
