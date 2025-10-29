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

pub struct ListBlockPlugin;

#[async_trait]
impl RecordPlugin for ListBlockPlugin {
    fn collection(&self) -> &str {
        "app.bsky.graph.listblock"
    }

    async fn insert(&self, pool: &Pool, uri: &str, cid: &str, record: &JsonValue, timestamp: &str) -> Result<(), IndexerError> {
        let client = pool.get().await?;
        let parts: Vec<&str> = uri.split('/').collect();
        let creator = parts.get(2).ok_or_else(|| IndexerError::InvalidUri(uri.to_string()))?;

        let subject = record.get("subject").and_then(|v| v.as_str());
        let created_at_str = record.get("createdAt").and_then(|v| v.as_str()).unwrap_or(timestamp);
        let created_at = parse_timestamp(created_at_str)?;
        let indexed_at = parse_timestamp(timestamp)?;

        client.execute(
            "INSERT INTO list_block (uri, cid, creator, subject_uri, created_at, indexed_at) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (uri) DO NOTHING",
            &[&uri, &cid, creator, &subject, &created_at, &indexed_at],
        ).await.map_err(|e| IndexerError::Database(e.into()))?;
        Ok(())
    }

    async fn update(&self, _pool: &Pool, _uri: &str, _cid: &str, _record: &JsonValue, _timestamp: &str) -> Result<(), IndexerError> {
        Ok(())
    }

    async fn delete(&self, pool: &Pool, uri: &str) -> Result<(), IndexerError> {
        let client = pool.get().await?;
        client.execute("DELETE FROM list_block WHERE uri = $1", &[&uri]).await.map_err(|e| IndexerError::Database(e.into()))?;
        Ok(())
    }
}
