use crate::indexing::RecordPlugin;
use crate::IndexerError;
use async_trait::async_trait;
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

        // Extract createdAt from record
        let created_at = record.get("createdAt").and_then(|c| c.as_str());

        // Check for duplicate (creator + subjectDid)
        if let (Some(follow_creator), Some(follow_subject)) = (&creator, subject_did) {
            let existing = client
                .query_opt(
                    "SELECT uri FROM follow WHERE creator = $1 AND subjectDid = $2",
                    &[&follow_creator, &follow_subject],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;

            if existing.is_some() {
                // Duplicate found, skip insert
                return Ok(());
            }
        }

        // Insert follow
        client
            .execute(
                r#"INSERT INTO follow (uri, cid, creator, subjectDid, createdAt, indexedAt, sortAt)
                   VALUES ($1, $2, $3, $4, $5, $6, $7)
                   ON CONFLICT (uri) DO NOTHING"#,
                &[&uri, &cid, &creator, &subject_did, &created_at, &timestamp, &timestamp],
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
                        &timestamp,
                    ],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

        // Update aggregates: profile_agg.followersCount for subjectDid
        if let Some(follow_subject) = subject_did {
            client
                .execute(
                    r#"INSERT INTO profile_agg (did, followersCount)
                       VALUES ($1, (SELECT COUNT(*) FROM follow WHERE subjectDid = $1))
                       ON CONFLICT (did) DO UPDATE SET followersCount = EXCLUDED.followersCount"#,
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
                    r#"INSERT INTO profile_agg (did, followsCount)
                       VALUES ($1, (SELECT COUNT(*) FROM follow WHERE creator = $1))
                       ON CONFLICT (did) DO UPDATE SET followsCount = EXCLUDED.followsCount"#,
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
                "SELECT creator, subjectDid FROM follow WHERE uri = $1",
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
                    r#"INSERT INTO profile_agg (did, followersCount)
                       VALUES ($1, (SELECT COUNT(*) FROM follow WHERE subjectDid = $1))
                       ON CONFLICT (did) DO UPDATE SET followersCount = EXCLUDED.followersCount"#,
                    &[&follow_subject],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

        // Update aggregates: profile_agg.followsCount for creator
        if let Some(follow_creator) = creator {
            client
                .execute(
                    r#"INSERT INTO profile_agg (did, followsCount)
                       VALUES ($1, (SELECT COUNT(*) FROM follow WHERE creator = $1))
                       ON CONFLICT (did) DO UPDATE SET followsCount = EXCLUDED.followsCount"#,
                    &[&follow_creator],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

        Ok(())
    }
}
