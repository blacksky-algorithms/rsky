use crate::indexing::RecordPlugin;
use crate::IndexerError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use deadpool_postgres::Pool;
use serde_json::Value as JsonValue;

/// Parse RFC3339 timestamp string into DateTime<Utc>
fn parse_timestamp(timestamp: &str) -> Result<DateTime<Utc>, IndexerError> {
    DateTime::parse_from_rfc3339(timestamp)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| IndexerError::Serialization(format!("Invalid timestamp '{}': {}", timestamp, e)))
}

pub struct PostGatePlugin;

#[async_trait]
impl RecordPlugin for PostGatePlugin {
    fn collection(&self) -> &str {
        "app.bsky.feed.postgate"
    }

    async fn insert(&self, pool: &Pool, uri: &str, cid: &str, record: &JsonValue, timestamp: &str) -> Result<(), IndexerError> {
        let client = pool.get().await?;
        let post_uri = record.get("post").and_then(|v| v.as_str());
        let created_at_str = record.get("createdAt").and_then(|v| v.as_str()).unwrap_or(timestamp);
        let created_at = parse_timestamp(created_at_str)?;
        let indexed_at = parse_timestamp(timestamp)?;

        client.execute(
            "INSERT INTO post_gate (uri, cid, post_uri, created_at, indexed_at) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (uri) DO NOTHING",
            &[&uri, &cid, &post_uri, &created_at, &indexed_at],
        ).await.map_err(|e| IndexerError::Database(e.into()))?;
        Ok(())
    }

    async fn update(&self, pool: &Pool, uri: &str, cid: &str, record: &JsonValue, timestamp: &str) -> Result<(), IndexerError> {
        self.insert(pool, uri, cid, record, timestamp).await
    }

    async fn delete(&self, pool: &Pool, uri: &str) -> Result<(), IndexerError> {
        let client = pool.get().await?;
        client.execute("DELETE FROM post_gate WHERE uri = $1", &[&uri]).await.map_err(|e| IndexerError::Database(e.into()))?;
        Ok(())
    }
}
