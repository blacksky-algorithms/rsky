use crate::indexing::RecordPlugin;
use crate::IndexerError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use deadpool_postgres::Pool;
use serde_json::Value as JsonValue;

pub struct RepostPlugin;

impl RepostPlugin {
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
impl RecordPlugin for RepostPlugin {
    fn collection(&self) -> &str {
        "app.bsky.feed.repost"
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

        // Extract subject from record
        let subject = record.get("subject").and_then(|s| s.get("uri")).and_then(|u| u.as_str());
        let subject_cid = record.get("subject").and_then(|s| s.get("cid")).and_then(|c| c.as_str());

        // Extract via from record (repost that led to this repost)
        let via = record.get("via").and_then(|v| v.get("uri")).and_then(|u| u.as_str());
        let via_cid = record.get("via").and_then(|v| v.get("cid")).and_then(|c| c.as_str());

        // Parse timestamps
        let indexed_at = Self::parse_timestamp(timestamp)?;
        let created_at = match record.get("createdAt").and_then(|c| c.as_str()) {
            Some(ts) => Self::parse_timestamp(ts)?,
            None => indexed_at.clone(),
        };

        // Calculate sortAt (MIN(indexedAt, createdAt))
        let sort_at = if created_at < indexed_at {
            created_at.clone()
        } else {
            indexed_at.clone()
        };

        // Check for duplicate (creator + subject)
        if let (Some(repost_creator), Some(repost_subject)) = (&creator, subject) {
            let existing = client
                .query_opt(
                    "SELECT uri FROM repost WHERE creator = $1 AND subject = $2",
                    &[&repost_creator, &repost_subject],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;

            if existing.is_some() {
                // Duplicate found, skip insert
                return Ok(());
            }
        }

        // Insert repost
        client
            .execute(
                r#"INSERT INTO repost (uri, cid, creator, subject, subject_cid, via, via_cid, created_at, indexed_at, sort_at)
                   VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                   ON CONFLICT (uri) DO NOTHING"#,
                &[&uri, &cid, &creator, &subject, &subject_cid, &via, &via_cid, &created_at, &indexed_at, &sort_at],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Insert into feed_item table
        client
            .execute(
                r#"INSERT INTO feed_item (type, uri, cid, post_uri, originator_did, sort_at)
                   VALUES ($1, $2, $3, $4, $5, $6)
                   ON CONFLICT (uri, cid) DO NOTHING"#,
                &[&"repost", &uri, &cid, &subject, &creator, &sort_at],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Create notifications
        if let (Some(repost_creator), Some(repost_subject)) = (&creator, subject) {
            let subject_creator = Self::extract_creator(repost_subject);

            // Notification to subject author (prevent self-notifications)
            if subject_creator.as_ref() != Some(repost_creator) {
                if let Some(notif_recipient) = subject_creator {
                    client
                        .execute(
                            r#"INSERT INTO notification (did, author, record_uri, record_cid, reason, reason_subject, sort_at)
                               VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
                            &[
                                &notif_recipient,
                                &repost_creator,
                                &uri,
                                &cid,
                                &"repost",
                                &Some(repost_subject),
                                &sort_at,
                            ],
                        )
                        .await
                        .map_err(|e| IndexerError::Database(e.into()))?;
                }
            }

            // Notification to via author if via exists (prevent self-notifications)
            if let Some(via_uri) = via {
                let via_creator = Self::extract_creator(via_uri);
                if via_creator.as_ref() != Some(repost_creator) {
                    if let Some(notif_recipient) = via_creator {
                        client
                            .execute(
                                r#"INSERT INTO notification (did, author, record_uri, record_cid, reason, reason_subject, sort_at)
                                   VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
                                &[
                                    &notif_recipient,
                                    &repost_creator,
                                    &uri,
                                    &cid,
                                    &"repost-via-repost",
                                    &Some(via_uri),
                                    &sort_at,
                                ],
                            )
                            .await
                            .map_err(|e| IndexerError::Database(e.into()))?;
                    }
                }
            }
        }

        // Update aggregates: post_agg.repostCount
        if let Some(repost_subject) = subject {
            client
                .execute(
                    r#"INSERT INTO post_agg (uri, repost_count)
                       VALUES ($1, (SELECT COUNT(*) FROM repost WHERE subject = $1))
                       ON CONFLICT (uri) DO UPDATE SET repost_count = EXCLUDED.repost_count"#,
                    &[&repost_subject],
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
        // No-op for repost (immutable once created)
        Ok(())
    }

    async fn delete(&self, pool: &Pool, uri: &str) -> Result<(), IndexerError> {
        let client = pool.get().await?;

        // Get the repost data before deleting for aggregate updates
        let row = client
            .query_opt("SELECT subject FROM repost WHERE uri = $1", &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        let subject: Option<String> = row.and_then(|r| r.get(0));

        // Delete repost
        client
            .execute("DELETE FROM repost WHERE uri = $1", &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Delete from feed_item
        client
            .execute("DELETE FROM feed_item WHERE uri = $1", &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Delete notifications for this repost
        client
            .execute("DELETE FROM notification WHERE record_uri = $1", &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Update aggregates: post_agg.repostCount
        if let Some(repost_subject) = subject {
            client
                .execute(
                    r#"INSERT INTO post_agg (uri, repost_count)
                       VALUES ($1, (SELECT COUNT(*) FROM repost WHERE subject = $1))
                       ON CONFLICT (uri) DO UPDATE SET repost_count = EXCLUDED.repost_count"#,
                    &[&repost_subject],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
        }

        Ok(())
    }
}
