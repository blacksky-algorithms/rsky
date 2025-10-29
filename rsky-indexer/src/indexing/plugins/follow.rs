use crate::indexing::RecordPlugin;
use crate::IndexerError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use deadpool_postgres::Pool;
use serde_json::Value as JsonValue;

pub struct FollowPlugin;

impl FollowPlugin {
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
impl RecordPlugin for FollowPlugin {
    fn collection(&self) -> &str {
        "app.bsky.graph.follow"
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

        // Extract subjectDid from record
        let subject_did = record.get("subject").and_then(|s| s.as_str());

        // Parse timestamps
        let indexed_at = Self::parse_timestamp(timestamp)?;
        let created_at = match record.get("createdAt").and_then(|c| c.as_str()) {
            Some(ts) => Self::parse_timestamp(ts)?,
            None => indexed_at.clone(),
        };

        // Check for duplicate (creator + subjectDid)
        if let (Some(follow_creator), Some(follow_subject)) = (&creator, subject_did) {
            let existing = client
                .query_opt(
                    r#"SELECT uri FROM follow WHERE creator = $1 AND subject_did = $2"#,
                    &[&follow_creator, &follow_subject],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;

            if existing.is_some() {
                // Duplicate found, skip insert
                return Ok(());
            }
        }

        // Calculate sort_at for tables without auto-generated columns
        let sort_at = if created_at < indexed_at {
            created_at.clone()
        } else {
            indexed_at.clone()
        };

        // Insert follow
        client
            .execute(
                r#"INSERT INTO follow (uri, cid, creator, subject_did, created_at, indexed_at, sort_at)
                   VALUES ($1, $2, $3, $4, $5, $6, $7)
                   ON CONFLICT (uri) DO NOTHING"#,
                &[&uri, &cid, &creator, &subject_did, &created_at, &indexed_at, &sort_at],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Create notification to subjectDid
        if let (Some(follow_creator), Some(follow_subject)) = (&creator, subject_did) {
            client
                .execute(
                    r#"INSERT INTO notification (did, author, record_uri, record_cid, reason, reason_subject, sort_at)
                       VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
                    &[
                        &follow_subject,
                        &follow_creator,
                        &uri,
                        &cid,
                        &"follow",
                        &Option::<&str>::None,
                        &indexed_at,
                    ],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

        // Update aggregates: profile_agg.followersCount for subjectDid
        if let Some(follow_subject) = subject_did {
            client
                .execute(
                    r#"INSERT INTO profile_agg (did, followers_count)
                       VALUES ($1, (SELECT COUNT(*) FROM follow WHERE subject_did = $1))
                       ON CONFLICT (did) DO UPDATE SET followers_count = EXCLUDED.followers_count"#,
                    &[&follow_subject],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

        // Update aggregates: profile_agg.followsCount for creator
        // Note: TypeScript uses explicit locking (coalesceWithLock) to avoid thrash during backfills
        // For now, we implement without locking - this may cause contention during high-volume backfills
        if let Some(follow_creator) = &creator {
            client
                .execute(
                    r#"INSERT INTO profile_agg (did, follows_count)
                       VALUES ($1, (SELECT COUNT(*) FROM follow WHERE creator = $1))
                       ON CONFLICT (did) DO UPDATE SET follows_count = EXCLUDED.follows_count"#,
                    &[&follow_creator],
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
        // No-op for follow (immutable once created)
        Ok(())
    }

    async fn delete(&self, pool: &Pool, uri: &str) -> Result<(), IndexerError> {
        let client = pool.get().await?;

        // Get the follow data before deleting for aggregate updates
        let row = client
            .query_opt(
                r#"SELECT creator, subject_did FROM follow WHERE uri = $1"#,
                &[&uri],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        let (creator, subject_did): (Option<String>, Option<String>) = row
            .map(|r| (r.get(0), r.get(1)))
            .unwrap_or((None, None));

        // Delete follow
        client
            .execute("DELETE FROM follow WHERE uri = $1", &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Delete notifications for this follow
        client
            .execute("DELETE FROM notification WHERE record_uri = $1", &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Update aggregates: profile_agg.followersCount for subjectDid
        if let Some(follow_subject) = subject_did {
            client
                .execute(
                    r#"INSERT INTO profile_agg (did, followers_count)
                       VALUES ($1, (SELECT COUNT(*) FROM follow WHERE subject_did = $1))
                       ON CONFLICT (did) DO UPDATE SET followers_count = EXCLUDED.followers_count"#,
                    &[&follow_subject],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

        // Update aggregates: profile_agg.followsCount for creator
        if let Some(follow_creator) = creator {
            client
                .execute(
                    r#"INSERT INTO profile_agg (did, follows_count)
                       VALUES ($1, (SELECT COUNT(*) FROM follow WHERE creator = $1))
                       ON CONFLICT (did) DO UPDATE SET follows_count = EXCLUDED.follows_count"#,
                    &[&follow_creator],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

        Ok(())
    }
}
