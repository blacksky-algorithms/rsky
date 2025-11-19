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
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    let cursor_key = format!("backfill_enum:{relay_host}");
    let mut cursor = storage.get_cursor(&cursor_key)?.map(|c| c.to_string());

    let clean_hostname = relay_host
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');

    loop {
        let mut url = url::Url::parse(&format!(
            "https://{clean_hostname}/xrpc/com.atproto.sync.listRepos"
        ))
        .map_err(|e| WintermuteError::Other(format!("invalid url: {e}")))?;

        url.query_pairs_mut().append_pair("limit", "1000");
        if let Some(ref c) = cursor {
            url.query_pairs_mut().append_pair("cursor", c);
        }

        tracing::info!("fetching repos from {url}");

        let response = http_client.get(url.as_str()).send().await?;

        if !response.status().is_success() {
            return Err(WintermuteError::Other(format!(
                "http error: {}",
                response.status()
            )));
        }

        let list_response: ListReposResponse = response.json().await?;

        tracing::info!("received {} repos", list_response.repos.len());

        for repo in &list_response.repos {
            let job = BackfillJob {
                did: repo.did.clone(),
                retry_count: 0,
            };
            storage.enqueue_backfill(&job)?;
        }

        if let Some(next_cursor) = list_response.cursor {
            storage.set_cursor(&cursor_key, next_cursor.parse::<i64>().unwrap_or(0))?;
            cursor = Some(next_cursor);
        } else {
            tracing::info!("backfill queue population complete for {relay_host}");
            break;
        }
    }

    Ok(())
}
