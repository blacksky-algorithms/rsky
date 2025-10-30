use crate::indexing::RecordPlugin;
use crate::IndexerError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use deadpool_postgres::Pool;
use serde_json::Value as JsonValue;

pub struct ListItemPlugin;

impl ListItemPlugin {
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
impl RecordPlugin for ListItemPlugin {
    fn collection(&self) -> &str {
        "app.bsky.graph.listitem"
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

        // Extract subjectDid and listUri from record
        let subject_did = record.get("subject").and_then(|v| v.as_str());
        let list_uri = record.get("list").and_then(|v| v.as_str());

        // Validate that the listUri has the same creator as the list item
        if let (Some(item_creator), Some(list)) = (&creator, list_uri) {
            let list_creator = Self::extract_creator(list);
            if list_creator.as_ref() != Some(item_creator) {
                return Err(IndexerError::Serialization(
                    "Creator of listitem does not match creator of list".to_string(),
                ));
            }
        }

        // Check for duplicate (list_uri + subject_did)
        if let (Some(list), Some(subject)) = (list_uri, subject_did) {
            let existing = client
                .query_opt(
                    r#"SELECT uri FROM list_item WHERE "listUri" = $1 AND "subjectDid" = $2"#,
                    &[&list, &subject],
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
                r#"INSERT INTO list_item (uri, cid, creator, "subjectDid", "listUri", "createdAt", "indexedAt")
                   VALUES ($1, $2, $3, $4, $5, $6, $7)
                   ON CONFLICT (uri) DO NOTHING"#,
                &[&uri, &cid, &creator, &subject_did, &list_uri, &created_at.to_rfc3339(), &indexed_at.to_rfc3339()],
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
        // No-op for list_item (immutable once created)
        Ok(())
    }

    async fn delete(&self, pool: &Pool, uri: &str) -> Result<(), IndexerError> {
        let client = pool.get().await?;
        client
            .execute("DELETE FROM list_item WHERE uri = $1", &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;
        Ok(())
    }
}
