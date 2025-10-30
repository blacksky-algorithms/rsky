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

    /// Hash a string to i64 for PostgreSQL advisory lock
    /// Uses a simple hash function similar to Java's hashCode
    fn hash_lock_key(key: &str) -> i64 {
        let mut hash: i64 = 0;
        for byte in key.bytes() {
            hash = hash.wrapping_mul(31).wrapping_add(byte as i64);
        }
        hash
    }

    /// Execute aggregate update with coalescing lock to avoid thrashing during backfills
    /// Matches TypeScript's coalesceWithLock pattern
    /// If lock cannot be acquired, skip the update (another transaction is handling it)
    async fn update_with_coalesce_lock(
        pool: &Pool,
        lock_key: &str,
        did: &str,
        query: &str,
    ) -> Result<(), IndexerError> {
        // Get a connection from the pool
        let mut client = pool.get().await?;

        // Begin transaction
        let txn = client
            .transaction()
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Try to acquire advisory lock (auto-released at transaction end)
        let lock_id = Self::hash_lock_key(lock_key);
        let lock_acquired: bool = txn
            .query_one("SELECT pg_try_advisory_xact_lock($1)", &[&lock_id])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?
            .get(0);

        if lock_acquired {
            // Lock acquired, perform the update
            txn.execute(query, &[&did])
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;

            // Commit transaction (releases lock)
            txn.commit()
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        } else {
            // Lock not acquired, another transaction is handling it
            // Rollback and skip (coalescing behavior)
            txn.rollback()
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

        Ok(())
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
                    r#"SELECT uri FROM follow WHERE creator = $1 AND "subjectDid" = $2"#,
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
                r#"INSERT INTO follow (uri, cid, creator, "subjectDid", "createdAt", "indexedAt")
                   VALUES ($1, $2, $3, $4, $5, $6, $7)
                   ON CONFLICT (uri) DO NOTHING"#,
                &[&uri, &cid, &creator, &subject_did, &created_at.to_rfc3339(), &indexed_at.to_rfc3339()],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Create notification to subjectDid
        if let (Some(follow_creator), Some(follow_subject)) = (&creator, subject_did) {
            client
                .execute(
                    r#"INSERT INTO notification (did, author, "recordUri", "recordCid", reason, "reasonSubject")
                       VALUES ($1, $2, $3, $4, $5, $6)"#,
                    &[
                        &follow_subject,
                        &follow_creator,
                        &uri,
                        &cid,
                        &"follow",
                        &Option::<&str>::None
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
                       VALUES ($1, (SELECT COUNT(*) FROM follow WHERE "subjectDid" = $1))
                       ON CONFLICT (did) DO UPDATE SET followers_count = EXCLUDED.followers_count"#,
                    &[&follow_subject],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

        // Update aggregates: profile_agg.followsCount for creator
        // Use explicit locking (coalesceWithLock) to avoid thrash during backfills
        if let Some(follow_creator) = &creator {
            let lock_key = format!("followsCount:{}", follow_creator);
            let query = r#"INSERT INTO profile_agg (did, follows_count)
                          VALUES ($1, (SELECT COUNT(*) FROM follow WHERE creator = $1))
                          ON CONFLICT (did) DO UPDATE SET follows_count = EXCLUDED.follows_count"#;

            Self::update_with_coalesce_lock(pool, &lock_key, follow_creator, query).await?;
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
                r#"SELECT creator, "subjectDid" FROM follow WHERE uri = $1"#,
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
            .execute(r#"DELETE FROM notification WHERE "recordUri" = $1"#, &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Update aggregates: profile_agg.followersCount for subjectDid
        if let Some(follow_subject) = subject_did {
            client
                .execute(
                    r#"INSERT INTO profile_agg (did, followers_count)
                       VALUES ($1, (SELECT COUNT(*) FROM follow WHERE "subjectDid" = $1))
                       ON CONFLICT (did) DO UPDATE SET followers_count = EXCLUDED.followers_count"#,
                    &[&follow_subject],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

        // Update aggregates: profile_agg.followsCount for creator
        // Use explicit locking (coalesceWithLock) to avoid thrash during backfills
        if let Some(follow_creator) = creator {
            let lock_key = format!("followsCount:{}", follow_creator);
            let query = r#"INSERT INTO profile_agg (did, follows_count)
                          VALUES ($1, (SELECT COUNT(*) FROM follow WHERE creator = $1))
                          ON CONFLICT (did) DO UPDATE SET follows_count = EXCLUDED.follows_count"#;

            Self::update_with_coalesce_lock(pool, &lock_key, &follow_creator, query).await?;
        }

        Ok(())
    }
}
