pub mod plugins;

use crate::{did_helpers, IndexerError};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use deadpool_postgres::Pool;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::debug;

/// Parse RFC3339 timestamp string into DateTime<Utc>
fn parse_timestamp(timestamp: &str) -> Result<DateTime<Utc>, IndexerError> {
    DateTime::parse_from_rfc3339(timestamp)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| IndexerError::Serialization(format!("Invalid timestamp '{}': {}", timestamp, e)))
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
    async fn delete(
        &self,
        pool: &Pool,
        uri: &str,
    ) -> Result<(), IndexerError>;
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
        plugins.insert(list_block_plugin.collection().to_string(), list_block_plugin);

        let feed_generator_plugin = Arc::new(plugins::FeedGeneratorPlugin);
        plugins.insert(feed_generator_plugin.collection().to_string(), feed_generator_plugin);

        let labeler_plugin = Arc::new(plugins::LabelerPlugin);
        plugins.insert(labeler_plugin.collection().to_string(), labeler_plugin);

        let starter_pack_plugin = Arc::new(plugins::StarterPackPlugin);
        plugins.insert(starter_pack_plugin.collection().to_string(), starter_pack_plugin);

        let thread_gate_plugin = Arc::new(plugins::ThreadGatePlugin);
        plugins.insert(thread_gate_plugin.collection().to_string(), thread_gate_plugin);

        let post_gate_plugin = Arc::new(plugins::PostGatePlugin);
        plugins.insert(post_gate_plugin.collection().to_string(), post_gate_plugin);

        let verification_plugin = Arc::new(plugins::VerificationPlugin);
        plugins.insert(verification_plugin.collection().to_string(), verification_plugin);

        let status_plugin = Arc::new(plugins::StatusPlugin);
        plugins.insert(status_plugin.collection().to_string(), status_plugin);

        let chat_declaration_plugin = Arc::new(plugins::ChatDeclarationPlugin);
        plugins.insert(chat_declaration_plugin.collection().to_string(), chat_declaration_plugin);

        let notif_declaration_plugin = Arc::new(plugins::NotifDeclarationPlugin);
        plugins.insert(notif_declaration_plugin.collection().to_string(), notif_declaration_plugin);

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
        _rev: &str,
        _opts: IndexingOptions,
    ) -> Result<(), IndexerError> {
        // Parse the URI to get collection
        let parts: Vec<&str> = uri.split('/').collect();
        if parts.len() < 3 {
            return Err(IndexerError::InvalidUri(uri.to_string()));
        }

        let collection = parts[parts.len() - 2];
        let did = parts.first().ok_or_else(|| IndexerError::InvalidUri(uri.to_string()))?;

        // First, update the generic record table
        match action {
            WriteOpAction::Create | WriteOpAction::Update => {
                self.upsert_record(uri, cid, did, record, timestamp).await?;
            }
            WriteOpAction::Delete => {
                self.delete_record_generic(uri).await?;
            }
        }

        // Then, handle collection-specific indexing via plugins
        if let Some(plugin) = self.plugins.get(collection) {
            match action {
                WriteOpAction::Create => {
                    plugin.insert(&self.pool, uri, cid, record, timestamp).await?;
                }
                WriteOpAction::Update => {
                    plugin.update(&self.pool, uri, cid, record, timestamp).await?;
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
                INSERT INTO actor_sync (did, commit_cid, repo_rev, last_seen)
                VALUES ($1, $2, $3, NOW())
                ON CONFLICT (did) DO UPDATE
                SET commit_cid = EXCLUDED.commit_cid,
                    repo_rev = EXCLUDED.repo_rev,
                    last_seen = NOW()
                WHERE actor_sync.repo_rev < EXCLUDED.repo_rev
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
                    INSERT INTO actor (did, indexed_at)
                    VALUES ($1, $2)
                    ON CONFLICT (did) DO UPDATE
                    SET indexed_at = EXCLUDED.indexed_at
                    WHERE actor.indexed_at IS NULL OR actor.indexed_at < EXCLUDED.indexed_at
                    "#,
                    &[&did, &indexed_at],
                )
                .await
                .map_err(|e| IndexerError::Database(e.into()))?;
            return Ok(());
        };

        // Check if we need to reindex
        let client = self.pool.get().await?;
        let actor = client
            .query_opt("SELECT handle, indexed_at FROM actor WHERE did = $1", &[&did])
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
                Ok(Some(handle_to_did)) if handle_to_did == did => {
                    Some(handle.to_lowercase())
                }
                _ => None,
            }
        } else {
            None
        };

        // Handle contention: if another actor has this handle, clear it
        if let Some(ref handle) = verified_handle {
            let actor_with_handle = client
                .query_opt(
                    "SELECT did FROM actor WHERE handle = $1",
                    &[&handle],
                )
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
                INSERT INTO actor (did, handle, indexed_at)
                VALUES ($1, $2, $3)
                ON CONFLICT (did) DO UPDATE
                SET handle = EXCLUDED.handle,
                    indexed_at = EXCLUDED.indexed_at
                "#,
                &[&did, &verified_handle, &indexed_at],
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

        let indexed_at: Option<DateTime<Utc>> = row.get(1);
        let Some(indexed_at) = indexed_at else {
            return Ok(true);
        };

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
    /// - active=false with status in [deactivated, suspended, takendown] -> upstream_status=status
    pub async fn update_actor_status(
        &self,
        did: &str,
        active: bool,
        status: Option<String>,
    ) -> Result<(), IndexerError> {
        let client = self.pool.get().await?;

        let upstream_status: Option<&str> = if active {
            None
        } else {
            match status.as_deref() {
                Some("deactivated") | Some("suspended") | Some("takendown") => status.as_deref(),
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
                SET upstream_status = $2
                WHERE did = $1
                "#,
                &[&did, &upstream_status],
            )
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
    ) -> Result<(), IndexerError> {
        let client = self.pool.get().await?;
        let indexed_at = parse_timestamp(timestamp)?;

        client
            .execute(
                r#"
                INSERT INTO record (uri, cid, did, json, indexed_at)
                VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT (uri) DO UPDATE
                SET cid = EXCLUDED.cid,
                    json = EXCLUDED.json,
                    indexed_at = EXCLUDED.indexed_at
                "#,
                &[&uri, &cid, &did, record, &indexed_at],
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
                    indexed_at = NOW()
                WHERE uri = $1
                "#,
                &[&uri],
            )
            .await
            .map_err(|e| IndexerError::Database(e.into()))?;

        Ok(())
    }
}
