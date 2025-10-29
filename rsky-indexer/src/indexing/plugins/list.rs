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

pub struct ListPlugin;

#[async_trait]
impl RecordPlugin for ListPlugin {
    fn collection(&self) -> &str {
        "app.bsky.graph.list"
    }

    async fn insert(&self, pool: &Pool, uri: &str, cid: &str, record: &JsonValue, timestamp: &str) -> Result<(), IndexerError> {
        let client = pool.get().await?;
        let parts: Vec<&str> = uri.split('/').collect();
        let creator = parts.get(2).ok_or_else(|| IndexerError::InvalidUri(uri.to_string()))?;

        let name = record.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let purpose = record.get("purpose").and_then(|v| v.as_str());
        let description = record.get("description").and_then(|v| v.as_str());
        let created_at_str = record.get("createdAt").and_then(|v| v.as_str()).unwrap_or(timestamp);
        let created_at = parse_timestamp(created_at_str)?;
        let indexed_at = parse_timestamp(timestamp)?;

        client.execute(
            "INSERT INTO list (uri, cid, creator, name, purpose, description, created_at, indexed_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) ON CONFLICT (uri) DO NOTHING",
            &[&uri, &cid, creator, &name, &purpose, &description, &created_at, &indexed_at],
        ).await.map_err(|e| IndexerError::Database(e.into()))?;
        Ok(())
    }

    async fn update(&self, pool: &Pool, uri: &str, cid: &str, record: &JsonValue, timestamp: &str) -> Result<(), IndexerError> {
        self.insert(pool, uri, cid, record, timestamp).await
    }

    async fn delete(&self, pool: &Pool, uri: &str) -> Result<(), IndexerError> {
        let client = pool.get().await?;
        client.execute("DELETE FROM list WHERE uri = $1", &[&uri]).await.map_err(|e| IndexerError::Database(e.into()))?;
        Ok(())
    }
}
