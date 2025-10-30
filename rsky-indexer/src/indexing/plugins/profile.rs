use crate::indexing::RecordPlugin;
use crate::indexing::parse_timestamp;
use crate::IndexerError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use deadpool_postgres::Pool;
use serde_json::Value as JsonValue;

pub struct ProfilePlugin;

impl ProfilePlugin {
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


}

#[async_trait]
impl RecordPlugin for ProfilePlugin {
    fn collection(&self) -> &str {
        "app.bsky.actor.profile"
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
            // TypeScript returns null for non-self profile records, we'll skip them
            return Ok(());
        }

        let client = pool.get().await?;

        // Extract creator from URI
        let creator = Self::extract_creator(uri);

        // Extract profile fields from record
        let display_name = record.get("displayName").and_then(|d| d.as_str());
        let description = record.get("description").and_then(|d| d.as_str());
        let avatar_cid = record
            .get("avatar")
            .and_then(|a| a.get("ref"))
            .and_then(|r| r.as_str());
        let banner_cid = record
            .get("banner")
            .and_then(|b| b.get("ref"))
            .and_then(|r| r.as_str());

        // Extract joinedViaStarterPack.uri from record
        let joined_via_starter_pack_uri = record
            .get("joinedViaStarterPack")
            .and_then(|j| j.get("uri"))
            .and_then(|u| u.as_str());

        // Parse timestamps
        let indexed_at = parse_timestamp(timestamp)?;
        let created_at = match record.get("createdAt").and_then(|c| c.as_str()) {
            Some(ts) => parse_timestamp(ts)?,
            None => indexed_at.clone(),
        };

        client
            .execute(
                r#"INSERT INTO profile (uri, cid, creator, "displayName", description, "avatarCid", "bannerCid", "joinedViaStarterPackUri", "createdAt", "indexedAt")
                   VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                   ON CONFLICT (uri) DO NOTHING"#,
                &[
                    &uri,
                    &cid,
                    &creator,
                    &display_name,
                    &description,
                    &avatar_cid,
                    &banner_cid,
                    &joined_via_starter_pack_uri,
                    &created_at.to_rfc3339(),
                    &indexed_at.to_rfc3339(),
                ],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Create notification for starterpack-joined
        // If joinedViaStarterPackUri exists, notify the starter pack creator
        if let (Some(starter_pack_uri), Some(profile_creator)) =
            (joined_via_starter_pack_uri, creator.as_ref())
        {
            // Extract the starter pack creator from the starter pack URI
            let starter_pack_creator = Self::extract_creator(starter_pack_uri);

            if let Some(notif_recipient) = starter_pack_creator {
                client
                    .execute(
                        r#"INSERT INTO notification (did, author, "recordUri", "recordCid", reason, "reasonSubject", "sortAt")
                           VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
                        &[
                            &notif_recipient,
                            &profile_creator,
                            &uri,
                            &cid,
                            &"starterpack-joined",
                            &Some(starter_pack_uri),
                            &indexed_at.to_rfc3339(),
                        ],
                    )
                    .await
                    .map_err(|e| IndexerError::Database(e.into()))?;
            }
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
        // No-op for profile (immutable once created)
        Ok(())
    }

    async fn delete(&self, pool: &Pool, uri: &str) -> Result<(), IndexerError> {
        let client = pool.get().await?;
        client
            .execute("DELETE FROM profile WHERE uri = $1", &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;
        Ok(())
    }
}
