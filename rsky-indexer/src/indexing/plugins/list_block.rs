use crate::indexing::RecordPlugin;
use crate::IndexerError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use deadpool_postgres::Pool;
use serde_json::Value as JsonValue;

pub struct ListBlockPlugin;

impl ListBlockPlugin {
    /// Extract creator DID from AT URI (format: at://did:plc:xyz/collection/rkey)
    fn extract_creator(uri: &str) -> Option<String> {
        if let Some(stripped) = uri.strip_prefix("at://") {
            if let Some(did_end) = stripped.find('/') {
                return Some(stripped[..did_end].to_string());
            }
        }
        None
    }

    /// Parse ISO8601/RFC3339 timestamp string to DateTime<Utc>
    fn parse_timestamp(timestamp: &str) -> Result<DateTime<Utc>, IndexerError> {
        DateTime::parse_from_rfc3339(timestamp)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| IndexerError::Serialization(format!("Invalid timestamp '{}': {}", timestamp, e)))
    }
}

#[async_trait]
impl RecordPlugin for ListBlockPlugin {
    fn collection(&self) -> &str {
        "app.bsky.graph.listblock"
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

        // Extract creator from URI
        let creator = Self::extract_creator(uri);

        // Extract subject_uri from record
        let subject_uri = record.get("subject").and_then(|v| v.as_str());

        // Check for duplicate (creator + subject_uri)
        if let (Some(creator_did), Some(subject)) = (&creator, subject_uri) {
            let existing = client
                .query_opt(
                    "SELECT uri FROM list_block WHERE creator = $1 AND subject_uri = $2",
                    &[&creator_did, &subject],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;

            if existing.is_some() {
                // Duplicate found, skip insert
                return Ok(());
            }
        }

        // Parse timestamps
        let indexed_at = Self::parse_timestamp(timestamp)?;
        let created_at = match record.get("createdAt").and_then(|c| c.as_str()) {
            Some(ts) => Self::parse_timestamp(ts)?,
            None => indexed_at.clone(),
        };

        client
            .execute(
                r#"INSERT INTO list_block (uri, cid, creator, subject_uri, created_at, indexed_at)
                   VALUES ($1, $2, $3, $4, $5, $6)
                   ON CONFLICT (uri) DO NOTHING"#,
                &[&uri, &cid, &creator, &subject_uri, &created_at, &indexed_at],
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
        // No-op for list_block (immutable once created)
        Ok(())
    }

    async fn delete(&self, pool: &Pool, uri: &str) -> Result<(), IndexerError> {
        let client = pool.get().await?;
        client
            .execute("DELETE FROM list_block WHERE uri = $1", &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;
        Ok(())
    }
}
