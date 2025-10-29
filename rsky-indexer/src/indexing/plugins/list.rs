use crate::indexing::RecordPlugin;
use crate::IndexerError;
use async_trait::async_trait;
use deadpool_postgres::Pool;
use serde_json::Value as JsonValue;

pub struct ListPlugin;

impl ListPlugin {
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
impl RecordPlugin for ListPlugin {
    fn collection(&self) -> &str {
        "app.bsky.graph.list"
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

        // Extract fields from record
        let name = record.get("name").and_then(|v| v.as_str());
        let purpose = record.get("purpose").and_then(|v| v.as_str());
        let description = record.get("description").and_then(|v| v.as_str());

        // Extract descriptionFacets as JSON string if present
        let description_facets = record
            .get("descriptionFacets")
            .map(|v| v.to_string());

        // Extract avatarCid from avatar.ref if present
        let avatar_cid = record
            .get("avatar")
            .and_then(|a| a.get("ref"))
            .and_then(|r| r.as_str());

        // Extract createdAt from record
        let created_at = record.get("createdAt").and_then(|c| c.as_str());

        client
            .execute(
                r#"INSERT INTO list (uri, cid, creator, name, purpose, description, descriptionFacets, avatarCid, createdAt, indexedAt)
                   VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                   ON CONFLICT (uri) DO NOTHING"#,
                &[
                    &uri,
                    &cid,
                    &creator,
                    &name,
                    &purpose,
                    &description,
                    &description_facets,
                    &avatar_cid,
                    &created_at,
                    &timestamp,
                ],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;
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
        // List can be updated, treat as upsert
        self.insert(pool, uri, cid, record, timestamp).await
    }

    async fn delete(&self, pool: &Pool, uri: &str) -> Result<(), IndexerError> {
        let client = pool.get().await?;
        client
            .execute("DELETE FROM list WHERE uri = $1", &[&uri])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;
        Ok(())
    }
}
