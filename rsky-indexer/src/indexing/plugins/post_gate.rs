use crate::indexing::RecordPlugin;
use crate::IndexerError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use deadpool_postgres::Pool;
use serde_json::Value as JsonValue;

pub struct PostGatePlugin;

impl PostGatePlugin {
    /// Extract creator DID from AT URI (format: at://did:plc:xyz/collection/rkey)
    fn extract_creator(uri: &str) -> Option<String> {
        if let Some(stripped) = uri.strip_prefix("at://") {
            if let Some(did_end) = stripped.find('/') {
                return Some(stripped[..did_end].to_string());
            }
        }
        None
    }

    /// Extract rkey from AT URI (last part after final /)
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
impl RecordPlugin for PostGatePlugin {
    fn collection(&self) -> &str {
        "app.bsky.feed.postgate"
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

        // Extract postUri from record
        let post_uri = record.get("post").and_then(|v| v.as_str());

        // Validate that postUri creator and rkey match post gate creator and rkey
        if let (Some(gate_creator), Some(gate_rkey), Some(post)) =
            (creator.as_ref(), Self::extract_rkey(uri), post_uri) {
            let post_creator = Self::extract_creator(post);
            let post_rkey = Self::extract_rkey(post);

            if post_creator.as_ref() != Some(gate_creator) || post_rkey.as_deref() != Some(gate_rkey.as_str()) {
                return Err(IndexerError::Serialization(
                    "Creator and rkey of post gate does not match its post".to_string(),
                ));
            }
        }

        // Check for duplicate (post_uri)
        if let Some(post) = post_uri {
            let existing = client
                .query_opt(
                    r#"SELECT uri FROM post_gate WHERE "postUri" = $1"#,
                    &[&post],
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

        // Insert post gate
        client
            .execute(
                r#"INSERT INTO post_gate (uri, cid, creator, "postUri", "createdAt", "indexedAt")
                   VALUES ($1, $2, $3, $4, $5, $6)
                   ON CONFLICT (uri) DO NOTHING"#,
                &[&uri, &cid, &creator, &post_uri, &created_at.to_rfc3339(), &indexed_at.to_rfc3339()],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Update post to set has_post_gate = true
        if let Some(post) = post_uri {
            client
                .execute(
                    r#"UPDATE post SET "hasPostGate" = true WHERE uri = $1"#,
                    &[&post],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

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
        // Post gate can be updated, treat as upsert
        self.insert(pool, uri, cid, record, timestamp).await
    }

    async fn delete(&self, pool: &Pool, uri: &str) -> Result<(), IndexerError> {
        let client = pool.get().await?;

        // Get post_uri before deleting so we can update the post table
        let row = client
            .query_opt(r#"SELECT "postUri" FROM post_gate WHERE uri = $1"#, &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        let post_uri: Option<String> = row.and_then(|r| r.get(0));

        // Delete post gate
        client
            .execute("DELETE FROM post_gate WHERE uri = $1", &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Update post to set has_post_gate = false
        if let Some(post) = post_uri {
            client
                .execute(
                    r#"UPDATE post SET "hasPostGate" = false WHERE uri = $1"#,
                    &[&post],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

        Ok(())
    }
}
