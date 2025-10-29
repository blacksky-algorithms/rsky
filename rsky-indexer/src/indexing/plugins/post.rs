use crate::indexing::RecordPlugin;
use crate::IndexerError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use deadpool_postgres::Pool;
use serde_json::Value as JsonValue;
use tracing::debug;

/// Parse RFC3339 timestamp string into DateTime<Utc>
fn parse_timestamp(timestamp: &str) -> Result<DateTime<Utc>, IndexerError> {
    DateTime::parse_from_rfc3339(timestamp)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| IndexerError::Serialization(format!("Invalid timestamp '{}': {}", timestamp, e)))
}

pub struct PostPlugin;

#[async_trait]
impl RecordPlugin for PostPlugin {
    fn collection(&self) -> &str {
        "app.bsky.feed.post"
    }

    async fn insert(
        &self,
        pool: &Pool,
        uri: &str,
        cid: &str,
        record: &JsonValue,
        timestamp: &str,
    ) -> Result<(), IndexerError> {
        let client = pool.get().await?;

        let text = record.get("text").and_then(|v| v.as_str()).unwrap_or("");
        let created_at_str = record.get("createdAt").and_then(|v| v.as_str()).unwrap_or(timestamp);
        let created_at = parse_timestamp(created_at_str)?;
        let indexed_at = parse_timestamp(timestamp)?;
        let parts: Vec<&str> = uri.split('/').collect();
        let creator = parts.get(2).ok_or_else(|| IndexerError::InvalidUri(uri.to_string()))?;

        client
            .execute(
                "INSERT INTO post (uri, cid, creator, text, created_at, indexed_at) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (uri) DO NOTHING",
                &[&uri, &cid, creator, &text, &created_at, &indexed_at],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        debug!("Indexed post: {}", uri);
        Ok(())
    }

    async fn update(
        &self,
        pool: &Pool,
        uri: &str,
        cid: &str,
        record: &JsonValue,
        timestamp: &str,
    ) -> Result<(), IndexerError> {
        let client = pool.get().await?;
        let text = record.get("text").and_then(|v| v.as_str()).unwrap_or("");
        let indexed_at = parse_timestamp(timestamp)?;

        client
            .execute(
                "UPDATE post SET cid = $1, text = $2, indexed_at = $3 WHERE uri = $4",
                &[&cid, &text, &indexed_at, &uri],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        Ok(())
    }

    async fn delete(
        &self,
        pool: &Pool,
        uri: &str,
    ) -> Result<(), IndexerError> {
        let client = pool.get().await?;

        client
            .execute("DELETE FROM post WHERE uri = $1", &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        Ok(())
    }
}
