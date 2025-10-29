use crate::indexing::RecordPlugin;
use crate::IndexerError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use deadpool_postgres::Pool;
use serde_json::Value as JsonValue;

pub struct VerificationPlugin;

impl VerificationPlugin {
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
impl RecordPlugin for VerificationPlugin {
    fn collection(&self) -> &str {
        "app.bsky.actor.verification"
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

        // Extract rkey from URI
        let rkey = Self::extract_rkey(uri);

        // Extract fields from record
        let subject = record.get("subject").and_then(|s| s.as_str());
        let handle = record.get("handle").and_then(|h| h.as_str());
        let display_name = record.get("displayName").and_then(|d| d.as_str());

        // Parse timestamps
        let indexed_at = Self::parse_timestamp(timestamp)?;
        let created_at = match record.get("createdAt").and_then(|c| c.as_str()) {
            Some(ts) => Self::parse_timestamp(ts)?,
            None => indexed_at.clone(),
        };

        // Check for duplicate (subject + creator)
        if let (Some(verification_subject), Some(verification_creator)) = (subject, &creator) {
            let existing = client
                .query_opt(
                    "SELECT uri FROM verification WHERE subject = $1 AND creator = $2",
                    &[&verification_subject, &verification_creator],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;

            if existing.is_some() {
                // Duplicate found, skip insert
                return Ok(());
            }
        }

        // Calculate sorted_at for tables without auto-generated columns
        let sorted_at = if created_at < indexed_at {
            created_at.clone()
        } else {
            indexed_at.clone()
        };

        // Insert verification
        client
            .execute(
                r#"INSERT INTO verification (uri, cid, rkey, creator, subject, handle, display_name, created_at, indexed_at, sorted_at)
                   VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                   ON CONFLICT (uri) DO NOTHING"#,
                &[
                    &uri,
                    &cid,
                    &rkey,
                    &creator,
                    &subject,
                    &handle,
                    &display_name,
                    &created_at,
                    &indexed_at,
                    &sorted_at,
                ],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Create notification to subject (verified)
        if let (Some(verification_subject), Some(verification_creator)) = (subject, &creator) {
            client
                .execute(
                    r#"INSERT INTO notification (did, author, record_uri, record_cid, reason, reason_subject, sort_at)
                       VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
                    &[
                        &verification_subject,
                        &verification_creator,
                        &uri,
                        &cid,
                        &"verified",
                        &Option::<&str>::None,
                        &indexed_at,
                    ],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

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
        // No-op for verification (immutable once created)
        Ok(())
    }

    async fn delete(&self, pool: &Pool, uri: &str) -> Result<(), IndexerError> {
        let client = pool.get().await?;

        // Get verification data before deleting for notifications
        let row = client
            .query_opt(
                "SELECT subject, creator, cid FROM verification WHERE uri = $1",
                &[&uri],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        let (subject, creator, record_cid): (Option<String>, Option<String>, Option<String>) = row
            .map(|r| (r.get(0), r.get(1), r.get(2)))
            .unwrap_or((None, None, None));

        // Delete verification
        client
            .execute("DELETE FROM verification WHERE uri = $1", &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Create notification to subject (unverified) with current timestamp
        if let (Some(verification_subject), Some(verification_creator), Some(cid_value)) =
            (subject, creator, record_cid)
        {
            let current_timestamp = Utc::now();
            client
                .execute(
                    r#"INSERT INTO notification (did, author, record_uri, record_cid, reason, reason_subject, sort_at)
                       VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
                    &[
                        &verification_subject,
                        &verification_creator,
                        &uri,
                        &cid_value,
                        &"unverified",
                        &Option::<&str>::None,
                        &current_timestamp,
                    ],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

        Ok(())
    }
}
