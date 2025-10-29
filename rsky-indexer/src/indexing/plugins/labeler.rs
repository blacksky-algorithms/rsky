use crate::indexing::RecordPlugin;
use crate::IndexerError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use deadpool_postgres::Pool;
use serde_json::Value as JsonValue;

pub struct LabelerPlugin;

impl LabelerPlugin {
    /// Extract creator DID from AT URI (format: at://did:plc:xyz/collection/rkey)
    fn extract_creator(uri: &str) -> Option<String> {
        if let Some(stripped) = uri.strip_prefix("at://") {
            if let Some(did_end) = stripped.find('/') {
                return Some(stripped[..did_end].to_string());
            }
        }
        None
    }

    /// Extract rkey from AT URI
    fn extract_rkey(uri: &str) -> Option<String> {
        uri.rsplit('/').next().map(|s| s.to_string())
    }

    /// Parse ISO8601/RFC3339 timestamp string to DateTime<Utc>
    fn parse_timestamp(timestamp: &str) -> Result<DateTime<Utc>, IndexerError> {
        DateTime::parse_from_rfc3339(timestamp)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| IndexerError::Serialization(format!("Invalid timestamp '{}': {}", timestamp, e)))
    }
}

#[async_trait]
impl RecordPlugin for LabelerPlugin {
    fn collection(&self) -> &str {
        "app.bsky.labeler.service"
    }

    async fn insert(
        &self,
        pool: &Pool,
        uri: &str,
        cid: &str,
        record: &JsonValue,
        timestamp: &str,
    ) -> Result<(), IndexerError> {
        // Validate rkey === 'self'
        let rkey = Self::extract_rkey(uri);
        if rkey.as_deref() != Some("self") {
            return Err(IndexerError::Serialization(format!(
                "Labeler record must have rkey 'self', got: {:?}",
                rkey
            )));
        }

        let client = pool.get().await?;
        let creator = Self::extract_creator(uri);

        // Parse timestamps
        let indexed_at = Self::parse_timestamp(timestamp)?;
        let created_at = match record.get("createdAt").and_then(|c| c.as_str()) {
            Some(ts) => Self::parse_timestamp(ts)?,
            None => indexed_at.clone(),
        };

        client
            .execute(
                r#"INSERT INTO labeler (uri, cid, creator, created_at, indexed_at)
                   VALUES ($1, $2, $3, $4, $5)
                   ON CONFLICT (uri) DO NOTHING"#,
                &[&uri, &cid, &creator, &created_at, &indexed_at],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;
        Ok(())
    }

    async fn update(
        &self,
        _pool: &Pool,
        _uri: &str,
        _cid: &str,
        _record: &JsonValue,
        _timestamp: &str,
    ) -> Result<(), IndexerError> {
        // No-op for labeler (immutable once created)
        Ok(())
    }

    async fn delete(&self, pool: &Pool, uri: &str) -> Result<(), IndexerError> {
        let client = pool.get().await?;
        client
            .execute("DELETE FROM labeler WHERE uri = $1", &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;
        Ok(())
    }
}
