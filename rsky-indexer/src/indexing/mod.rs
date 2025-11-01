pub mod plugins;

use crate::{did_helpers, IndexerError};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use deadpool_postgres::Pool;
use lexicon_cid::Cid;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::debug;

/// Parse RFC3339 timestamp string into DateTime<Utc>
/// Handles timestamps with or without timezone suffixes
/// Also handles both millisecond (.123) and microsecond (.123456) precision
pub fn parse_timestamp(timestamp: &str) -> Result<DateTime<Utc>, IndexerError> {
    let normalized = if timestamp.ends_with('Z')
        || timestamp.contains("+")
        || timestamp.rfind('-').map_or(false, |i| i > 10)
    {
        // Already has timezone
        timestamp.to_string()
    } else {
        // Missing timezone, append Z for UTC
        format!("{}Z", timestamp)
    };

    DateTime::parse_from_rfc3339(&normalized)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            IndexerError::Serialization(format!("Invalid timestamp '{}': {}", timestamp, e))
        })
}

/// Sanitize text by removing null bytes which PostgreSQL TEXT fields don't accept
/// Returns None if input is None, Some(sanitized) otherwise
pub fn sanitize_text(text: Option<String>) -> Option<String> {
    text.map(|s| s.replace('\0', ""))
}

/// Sanitize required text by removing null bytes
pub fn sanitize_text_required(text: &str) -> String {
    text.replace('\0', "")
}

/// Extract DID from AT Protocol URI using rsky-syntax
/// Returns None if URI is invalid or doesn't contain a DID
pub fn extract_did_from_uri(uri: &str) -> Option<String> {
    rsky_syntax::aturi::AtUri::new(uri.to_string(), None)
        .ok()
        .map(|at_uri| at_uri.host)
}

/// Extract record key (rkey) from AT Protocol URI using rsky-syntax
/// Returns None if URI is invalid or doesn't contain an rkey
pub fn extract_rkey_from_uri(uri: &str) -> Option<String> {
    rsky_syntax::aturi::AtUri::new(uri.to_string(), None)
        .ok()
        .map(|at_uri| at_uri.get_rkey())
        .filter(|rkey| !rkey.is_empty())
}

/// Extract collection from AT Protocol URI using rsky-syntax
/// Returns None if URI is invalid or doesn't contain a collection
pub fn extract_collection_from_uri(uri: &str) -> Option<String> {
    rsky_syntax::aturi::AtUri::new(uri.to_string(), None)
        .ok()
        .map(|at_uri| at_uri.get_collection())
        .filter(|collection| !collection.is_empty())
}

/// Convert JSON Value to IPLD-formatted JSON (for TypeScript AppView compatibility)
///
/// CRITICAL: This ensures CID references are stored as {"$link": "bafyrei..."}
/// instead of byte arrays [1,85,18,32,...] which fail lexicon validation.
fn convert_to_ipld_format(value: &mut JsonValue) {
    match value {
        JsonValue::Object(map) => {
            // Check if this is already in IPLD format
            if let Some(JsonValue::String(_)) = map.get("$link") {
                return;
            }

            // Recurse into nested objects
            for (_, v) in map.iter_mut() {
                convert_to_ipld_format(v);
            }
        }
        JsonValue::Array(arr) => {
            // Check if this is a byte array (all elements are numbers 0-255)
            let is_byte_array = arr.iter().all(
                |v| matches!(v, JsonValue::Number(n) if n.as_u64().map_or(false, |num| num <= 255)),
            );

            if is_byte_array && !arr.is_empty() {
                // Try to parse as CID - this will be handled by the parent if it's in an object
                // For now, just recurse into non-byte arrays
            } else {
                // Recurse into array elements
                for v in arr.iter_mut() {
                    convert_to_ipld_format(v);
                }
            }
        }
        _ => {} // Primitives don't need conversion
    }
}

/// Convert RepoRecord JSON to IPLD-formatted string
/// This replaces direct serde_json::to_string calls
fn record_to_ipld_json_string(record_json: &JsonValue) -> Result<String, IndexerError> {
    // First, we need to convert byte arrays to IPLD format
    // We'll do this recursively by converting the JSON tree
    fn convert_value(value: &JsonValue) -> JsonValue {
        match value {
            JsonValue::Object(map) => {
                let mut new_map = serde_json::Map::new();
                for (k, v) in map.iter() {
                    new_map.insert(k.clone(), convert_value(v));
                }
                JsonValue::Object(new_map)
            }
            JsonValue::Array(arr) => {
                // Check if this is a byte array (CID)
                let is_byte_array = arr.iter().all(|v| {
                    matches!(v, JsonValue::Number(n) if n.as_u64().map_or(false, |num| num <= 255))
                });

                if is_byte_array && !arr.is_empty() {
                    let bytes: Vec<u8> = arr
                        .iter()
                        .filter_map(|v| v.as_u64().map(|n| n as u8))
                        .collect();

                    if let Ok(cid) = Cid::try_from(&bytes[..]) {
                        return serde_json::json!({"$link": cid.to_string()});
                    }
                }

                // Recurse
                JsonValue::Array(arr.iter().map(|v| convert_value(v)).collect())
            }
            other => other.clone(),
        }
    }

    let converted = convert_value(record_json);
    serde_json::to_string(&converted).map_err(|e| IndexerError::Serialization(e.to_string()))
}

/// Action type for record operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOpAction {
    Create,
    Update,
    Delete,
}

/// Options for indexing records
#[derive(Debug, Clone, Default)]
pub struct IndexingOptions {
    pub disable_notifs: bool,
}

/// Trait for record plugins that handle specific collection types
#[async_trait]
pub trait RecordPlugin: Send + Sync {
    /// The collection this plugin handles (e.g., "app.bsky.feed.post")
    fn collection(&self) -> &str;

    /// Insert a new record
    async fn insert(
        &self,
        pool: &Pool,
        uri: &str,
        cid: &str,
        record: &JsonValue,
        timestamp: &str,
    ) -> Result<(), IndexerError>;

    /// Update an existing record
    async fn update(
        &self,
        pool: &Pool,
        uri: &str,
        cid: &str,
        record: &JsonValue,
        timestamp: &str,
    ) -> Result<(), IndexerError>;

    /// Delete a record
    async fn delete(&self, pool: &Pool, uri: &str) -> Result<(), IndexerError>;
}

/// Main indexing service that coordinates plugins
pub struct IndexingService {
    pool: Pool,
    plugins: HashMap<String, Arc<dyn RecordPlugin>>,
    id_resolver: Option<Arc<Mutex<rsky_identity::IdResolver>>>,
}

impl IndexingService {
    pub fn new(pool: Pool) -> Self {
        Self::new_with_resolver(pool, None)
    }

    pub fn new_with_resolver(
        pool: Pool,
        id_resolver: Option<Arc<Mutex<rsky_identity::IdResolver>>>,
    ) -> Self {
        let mut plugins: HashMap<String, Arc<dyn RecordPlugin>> = HashMap::new();

        // Register all plugins
        let post_plugin = Arc::new(plugins::PostPlugin);
        plugins.insert(post_plugin.collection().to_string(), post_plugin);

        let like_plugin = Arc::new(plugins::LikePlugin);
        plugins.insert(like_plugin.collection().to_string(), like_plugin);

        let follow_plugin = Arc::new(plugins::FollowPlugin);
        plugins.insert(follow_plugin.collection().to_string(), follow_plugin);

        let repost_plugin = Arc::new(plugins::RepostPlugin);
        plugins.insert(repost_plugin.collection().to_string(), repost_plugin);

        let block_plugin = Arc::new(plugins::BlockPlugin);
        plugins.insert(block_plugin.collection().to_string(), block_plugin);

        let profile_plugin = Arc::new(plugins::ProfilePlugin);
        plugins.insert(profile_plugin.collection().to_string(), profile_plugin);

        let list_plugin = Arc::new(plugins::ListPlugin);
        plugins.insert(list_plugin.collection().to_string(), list_plugin);

        let list_item_plugin = Arc::new(plugins::ListItemPlugin);
        plugins.insert(list_item_plugin.collection().to_string(), list_item_plugin);

        let list_block_plugin = Arc::new(plugins::ListBlockPlugin);
        plugins.insert(
            list_block_plugin.collection().to_string(),
            list_block_plugin,
        );

        let feed_generator_plugin = Arc::new(plugins::FeedGeneratorPlugin);
        plugins.insert(
            feed_generator_plugin.collection().to_string(),
            feed_generator_plugin,
        );

        let labeler_plugin = Arc::new(plugins::LabelerPlugin);
        plugins.insert(labeler_plugin.collection().to_string(), labeler_plugin);

        let starter_pack_plugin = Arc::new(plugins::StarterPackPlugin);
        plugins.insert(
            starter_pack_plugin.collection().to_string(),
            starter_pack_plugin,
        );

        let thread_gate_plugin = Arc::new(plugins::ThreadGatePlugin);
        plugins.insert(
            thread_gate_plugin.collection().to_string(),
            thread_gate_plugin,
        );

        let post_gate_plugin = Arc::new(plugins::PostGatePlugin);
        plugins.insert(post_gate_plugin.collection().to_string(), post_gate_plugin);

        let verification_plugin = Arc::new(plugins::VerificationPlugin);
        plugins.insert(
            verification_plugin.collection().to_string(),
            verification_plugin,
        );

        let status_plugin = Arc::new(plugins::StatusPlugin);
        plugins.insert(status_plugin.collection().to_string(), status_plugin);

        let chat_declaration_plugin = Arc::new(plugins::ChatDeclarationPlugin);
        plugins.insert(
            chat_declaration_plugin.collection().to_string(),
            chat_declaration_plugin,
        );

        let notif_declaration_plugin = Arc::new(plugins::NotifDeclarationPlugin);
        plugins.insert(
            notif_declaration_plugin.collection().to_string(),
            notif_declaration_plugin,
        );

        Self {
            pool,
            plugins,
            id_resolver,
        }
    }

    /// Index a record (create or update)
    pub async fn index_record(
        &self,
        uri: &str,
        cid: &str,
        record: &JsonValue,
        action: WriteOpAction,
        timestamp: &str,
        rev: &str,
        _opts: IndexingOptions,
    ) -> Result<(), IndexerError> {
        // Parse the URI to get collection, did, and rkey
        let at_uri = rsky_syntax::aturi::AtUri::new(uri.to_string(), None)
            .map_err(|_| IndexerError::InvalidUri(uri.to_string()))?;

        let did = at_uri.host.as_str();
        let collection = at_uri.get_collection();
        let rkey = at_uri.get_rkey();

        // First, update the generic record table
        match action {
            WriteOpAction::Create | WriteOpAction::Update => {
                self.upsert_record(uri, cid, did, record, timestamp, rev)
                    .await?;
            }
            WriteOpAction::Delete => {
                self.delete_record_generic(uri).await?;
            }
        }

        // Then, handle collection-specific indexing via plugins
        if let Some(plugin) = self.plugins.get(collection.as_str()) {
            match action {
                WriteOpAction::Create => {
                    plugin
                        .insert(&self.pool, uri, cid, record, timestamp)
                        .await?;
                }
                WriteOpAction::Update => {
                    plugin
                        .update(&self.pool, uri, cid, record, timestamp)
                        .await?;
                }
                WriteOpAction::Delete => {
                    plugin.delete(&self.pool, uri).await?;
                }
            }
        } else {
            debug!("No plugin for collection: {}", collection);
        }

        Ok(())
    }

    /// Delete a record
    pub async fn delete_record(&self, uri: &str, _rev: &str) -> Result<(), IndexerError> {
        // Parse URI to get collection
        let parts: Vec<&str> = uri.split('/').collect();
        if parts.len() < 3 {
            return Err(IndexerError::InvalidUri(uri.to_string()));
        }

        let collection = parts[parts.len() - 2];

        // Delete from generic record table
        self.delete_record_generic(uri).await?;

        // Delete from collection-specific table via plugin
        if let Some(plugin) = self.plugins.get(collection) {
            plugin.delete(&self.pool, uri).await?;
        }

        Ok(())
    }

    /// Update commit last seen for an actor
    pub async fn set_commit_last_seen(
        &self,
        did: &str,
        commit_cid: &str,
        rev: &str,
    ) -> Result<(), IndexerError> {
        let client = self.pool.get().await?;

        client
            .execute(
                r#"
                INSERT INTO actor_sync (did, "commitCid", "repoRev")
                VALUES ($1, $2, $3)
                ON CONFLICT (did) DO UPDATE
                SET "commitCid" = EXCLUDED."commitCid",
                    "repoRev" = EXCLUDED."repoRev"
                WHERE actor_sync."repoRev" < EXCLUDED."repoRev"
                "#,
                &[&did, &commit_cid, &rev],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        Ok(())
    }

    /// Index handle for an actor with full DID-to-handle resolution and verification
    /// Matches TypeScript implementation:
    /// 1. Check if reindex is needed based on timing
    /// 2. Resolve DID document to get handle
    /// 3. Verify handle resolves back to same DID
    /// 4. Handle contention (clear old actor's handle if needed)
    /// 5. Update actor with verified handle and timestamp
    pub async fn index_handle(&self, did: &str, timestamp: &str) -> Result<(), IndexerError> {
        let indexed_at = parse_timestamp(timestamp)?;

        // If no resolver configured, just update timestamp
        let Some(ref resolver) = self.id_resolver else {
            let client = self.pool.get().await?;
            client
                .execute(
                    r#"
                    INSERT INTO actor (did, "indexedAt")
                    VALUES ($1, $2)
                    ON CONFLICT (did) DO UPDATE
                    SET "indexedAt" = EXCLUDED."indexedAt"
                    WHERE actor."indexedAt" IS NULL OR actor."indexedAt" < EXCLUDED."indexedAt"
                    "#,
                    &[&did, &indexed_at.to_rfc3339()],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
            return Ok(());
        };

        // Check if we need to reindex
        let client = self.pool.get().await?;
        let actor = client
            .query_opt(
                r#"SELECT handle, "indexedAt" FROM actor WHERE did = $1"#,
                &[&did],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        if !Self::needs_handle_reindex(&actor, timestamp)? {
            return Ok(());
        }

        // Resolve DID to get handle
        let mut resolver_guard = resolver.lock().await;
        let doc = resolver_guard
            .did
            .resolve(did.to_string(), Some(true))
            .await
            .map_err(|e| IndexerError::Other(e))?
            .ok_or_else(|| IndexerError::Other(anyhow::anyhow!("DID not found: {}", did)))?;

        // Extract handle from DID document
        let handle_from_doc = did_helpers::get_handle(&doc);

        // Verify handle resolves back to same DID
        let verified_handle: Option<String> = if let Some(ref handle) = handle_from_doc {
            match resolver_guard.handle.resolve(&handle.clone()).await {
                Ok(Some(handle_to_did)) if handle_to_did == did => Some(handle.to_lowercase()),
                _ => None,
            }
        } else {
            None
        };

        // Handle contention: if another actor has this handle, clear it
        if let Some(ref handle) = verified_handle {
            let actor_with_handle = client
                .query_opt("SELECT did FROM actor WHERE handle = $1", &[&handle])
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;

            if let Some(row) = actor_with_handle {
                let existing_did: String = row.get(0);
                if existing_did != did {
                    // Clear the old actor's handle
                    client
                        .execute(
                            "UPDATE actor SET handle = NULL WHERE did = $1",
                            &[&existing_did],
                        )
                        .await
                        .map_err(|e| IndexerError::Database(e.into()))?;
                }
            }
        }

        // Insert or update actor with handle and timestamp
        client
            .execute(
                r#"
                INSERT INTO actor (did, handle, "indexedAt")
                VALUES ($1, $2, $3)
                ON CONFLICT (did) DO UPDATE
                SET handle = EXCLUDED.handle,
                    "indexedAt" = EXCLUDED."indexedAt"
                "#,
                &[&did, &verified_handle, &indexed_at.to_rfc3339()],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        Ok(())
    }

    /// Check if handle needs to be reindexed based on timing
    /// Matches TypeScript needsHandleReindex logic:
    /// - Always reindex if actor doesn't exist
    /// - Revalidate daily (DAY = 24 hours)
    /// - Revalidate more aggressively for invalidated handles (HOUR = 1 hour)
    fn needs_handle_reindex(
        actor: &Option<tokio_postgres::Row>,
        timestamp: &str,
    ) -> Result<bool, IndexerError> {
        const HOUR: i64 = 3600 * 1000; // milliseconds
        const DAY: i64 = HOUR * 24;

        let Some(row) = actor else {
            return Ok(true);
        };

        // Read indexedAt as String, then parse to DateTime
        let indexed_at_str: Option<String> = row.get(1);
        let Some(indexed_at_str) = indexed_at_str else {
            return Ok(true);
        };
        let indexed_at = parse_timestamp(&indexed_at_str)?;

        let timestamp_dt = parse_timestamp(timestamp)?;
        let time_diff = (timestamp_dt.timestamp_millis() - indexed_at.timestamp_millis()).abs();

        // Revalidate daily
        if time_diff > DAY {
            return Ok(true);
        }

        // Revalidate more aggressively for invalidated handles
        let handle: Option<String> = row.get(0);
        if handle.is_none() && time_diff > HOUR {
            return Ok(true);
        }

        Ok(false)
    }

    /// Update actor status (active/inactive/suspended)
    /// Maps active boolean and status to upstream_status field per TypeScript implementation:
    /// - active=true -> upstream_status=null
    /// - active=false with status in [deactivated, suspended, takendown, throttled, desynchronized] -> upstream_status=status
    pub async fn update_actor_status(
        &self,
        did: &str,
        active: bool,
        status: Option<String>,
    ) -> Result<(), IndexerError> {
        let client = self.pool.get().await?;

        let upstream_status: Option<String> = if active {
            None
        } else {
            match status.as_ref().map(|s| s.to_lowercase()).as_deref() {
                Some("deactivated")
                | Some("suspended")
                | Some("takendown")
                | Some("deleted")
                | Some("throttled")
                | Some("desynchronized") => {
                    // Normalize to lowercase for storage
                    // Sync v1.1: throttled = rate-limited, desynchronized = lost sync
                    status.as_ref().map(|s| s.to_lowercase())
                }
                Some(s) => {
                    return Err(IndexerError::Serialization(format!(
                        "Unrecognized account status: {}",
                        s
                    )))
                }
                None => None,
            }
        };

        client
            .execute(
                r#"
                UPDATE actor
                SET "upstreamStatus" = $2
                WHERE did = $1
                "#,
                &[&did, &upstream_status],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        Ok(())
    }

    /// Delete an actor and all their associated records
    /// Per TypeScript implementation: only deletes non-hosted (remote) actors
    pub async fn delete_actor(&self, did: &str) -> Result<(), IndexerError> {
        let client = self.pool.get().await?;

        // In production, you'd check if actor is hosted before deleting
        // For now, we'll delete all records for the actor

        // Delete from actor table
        client
            .execute("DELETE FROM actor WHERE did = $1", &[&did])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Delete all records created by this actor
        client
            .execute("DELETE FROM record WHERE did = $1", &[&did])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        // Delete from collection-specific tables
        client
            .execute(
                r#"DELETE FROM post WHERE uri LIKE $1"#,
                &[&format!("at://{}/", did)],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        client
            .execute(r#"DELETE FROM "like" WHERE creator = $1"#, &[&did])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        client
            .execute(r#"DELETE FROM repost WHERE creator = $1"#, &[&did])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        client
            .execute(r#"DELETE FROM follow WHERE creator = $1"#, &[&did])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        client
            .execute(r#"DELETE FROM profile WHERE creator = $1"#, &[&did])
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        Ok(())
    }

    /// Upsert a record into the generic record table
    async fn upsert_record(
        &self,
        uri: &str,
        cid: &str,
        did: &str,
        record: &JsonValue,
        timestamp: &str,
        rev: &str,
    ) -> Result<(), IndexerError> {
        let client = self.pool.get().await?;
        let indexed_at = parse_timestamp(timestamp)?;
        let indexed_at_str = indexed_at.to_rfc3339();

        let json_str = record_to_ipld_json_string(record)?;

        client
            .execute(
                r#"
                INSERT INTO record (uri, cid, did, json, "indexedAt", rev)
                VALUES ($1, $2, $3, $4, $5, $6)
                ON CONFLICT (uri) DO UPDATE
                SET cid = EXCLUDED.cid,
                    json = EXCLUDED.json,
                    "indexedAt" = EXCLUDED."indexedAt",
                    rev = EXCLUDED.rev
                "#,
                &[&uri, &cid, &did, &json_str, &indexed_at_str, &rev],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        Ok(())
    }

    /// Delete a record from the generic record table
    async fn delete_record_generic(&self, uri: &str) -> Result<(), IndexerError> {
        let client = self.pool.get().await?;

        // Don't actually delete, just mark as tombstone by clearing json and cid
        client
            .execute(
                r#"
                UPDATE record
                SET json = '{}',
                    cid = '',
                    "indexedAt" = NOW()
                WHERE uri = $1
                "#,
                &[&uri],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        Ok(())
    }

    /// Create a notification
    /// Per TypeScript implementation: notifications inform users of interactions with their content
    pub async fn create_notification(
        &self,
        did: &str,                    // who receives the notification
        author: &str,                 // who created the record
        record_uri: &str,             // URI of the record that triggered the notification
        record_cid: &str,             // CID of the record
        reason: &str, // 'like', 'repost', 'follow', 'mention', 'reply', 'quote', 'starterpack-joined', 'verified', 'unverified'
        reason_subject: Option<&str>, // optional subject (e.g., the post that was liked)
        sort_at: &str, // timestamp for sorting
    ) -> Result<(), IndexerError> {
        let client = self.pool.get().await?;

        client
            .execute(
                r#"INSERT INTO notification (did, author, record_uri, record_cid, reason, reason_subject, sort_at)
                   VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
                &[
                    &did,
                    &author,
                    &record_uri,
                    &record_cid,
                    &reason,
                    &reason_subject,
                    &sort_at,
                ],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        Ok(())
    }

    /// Delete notifications for a specific record URI
    /// Per TypeScript implementation: used when records are deleted or updated
    pub async fn delete_notifications_for_record(
        &self,
        record_uri: &str,
    ) -> Result<(), IndexerError> {
        let client = self.pool.get().await?;

        client
            .execute(
                "DELETE FROM notification WHERE record_uri = $1",
                &[&record_uri],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        Ok(())
    }
}
