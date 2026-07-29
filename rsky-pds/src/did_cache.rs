// based on https://github.com/bluesky-social/atproto/blob/main/packages/pds/src/did-cache

use crate::background::BackgroundQueue;
use crate::db::migrator::{migrate_to_latest, Migration};
use crate::db::sqlite::Db;
use anyhow::Result;
use rsky_identity::types::{CacheResult, DidCache, DidDocument, GetDocFn};
use rusqlite::{params, OptionalExtension};
use std::path::Path;
use std::time::{Duration, SystemTime};

pub const DID_CACHE_DB_MIGRATIONS: &[Migration] = &[Migration {
    name: "001",
    sql: "\
    CREATE TABLE did_doc (\
        did TEXT PRIMARY KEY, \
        doc TEXT NOT NULL, \
        \"updatedAt\" INTEGER NOT NULL\
    );",
}];

pub async fn get_migrated_db(location: impl AsRef<Path>) -> Result<Db> {
    let db = Db::open(location)?;
    migrate_to_latest(&db, DID_CACHE_DB_MIGRATIONS).await?;
    Ok(db)
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("timestamp in millis since UNIX epoch")
        .as_millis()
}

/// SQLite-backed did document cache. Cache writes triggered by stale reads
/// happen out-of-band on the background queue, and cache failures are
/// logged rather than surfaced so resolution never depends on the cache.
#[derive(Debug, Clone)]
pub struct DidSqliteCache {
    db: Db,
    background_queue: BackgroundQueue,
    stale_ttl: Duration,
    max_ttl: Duration,
}

impl DidSqliteCache {
    pub fn new(
        db: Db,
        background_queue: BackgroundQueue,
        stale_ttl: Duration,
        max_ttl: Duration,
    ) -> Self {
        DidSqliteCache {
            db,
            background_queue,
            stale_ttl,
            max_ttl,
        }
    }

    async fn cache_did_internal(&self, did: String, doc: &DidDocument) -> Result<()> {
        let doc = serde_json::to_string(doc)?;
        let updated_at = now_millis() as i64;
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO did_doc (did, doc, \"updatedAt\") VALUES (?1, ?2, ?3) \
                     ON CONFLICT (did) DO UPDATE SET \
                     doc = excluded.doc, \"updatedAt\" = excluded.\"updatedAt\"",
                    params![did, doc, updated_at],
                )?;
                Ok(())
            })
            .await
    }

    async fn clear_entry_internal(&self, did: String) -> Result<()> {
        self.db
            .run(move |conn| {
                conn.execute("DELETE FROM did_doc WHERE did = ?1", params![did])?;
                Ok(())
            })
            .await
    }

    async fn check_cache_internal(&self, did: String) -> Result<Option<CacheResult>> {
        let row: Option<(String, i64)> = self
            .db
            .run({
                let did = did.clone();
                move |conn| {
                    Ok(conn
                        .query_row(
                            "SELECT doc, \"updatedAt\" FROM did_doc WHERE did = ?1",
                            params![did],
                            |row| Ok((row.get(0)?, row.get(1)?)),
                        )
                        .optional()?)
                }
            })
            .await?;
        let Some((doc, updated_at)) = row else {
            return Ok(None);
        };
        let doc: DidDocument = serde_json::from_str(&doc)?;
        let now = now_millis();
        let updated_at = updated_at as u128;
        let expired = now > updated_at + self.max_ttl.as_millis();
        let stale = now > updated_at + self.stale_ttl.as_millis();
        Ok(Some(CacheResult {
            did,
            doc,
            updated_at,
            stale,
            expired,
        }))
    }

    /// Waits for queued cache refreshes to settle. Used in tests.
    pub async fn process_all(&self) {
        self.background_queue.process_all().await;
    }
}

#[async_trait::async_trait]
impl DidCache for DidSqliteCache {
    async fn cache_did(&self, did: String, doc: DidDocument) -> Result<()> {
        if let Err(err) = self.cache_did_internal(did.clone(), &doc).await {
            tracing::error!(%did, ?err, "failed to cache did");
        }
        Ok(())
    }

    async fn refresh_cache(&self, did: String, get_doc: GetDocFn) -> Result<()> {
        let cache = self.clone();
        self.background_queue.add(async move {
            match get_doc().await {
                Ok(Some(doc)) => cache.cache_did(did, doc).await,
                Ok(None) => cache.clear_entry(did).await,
                Err(err) => {
                    tracing::error!(%did, ?err, "refreshing did cache failed");
                    Ok(())
                }
            }
        });
        Ok(())
    }

    async fn check_cache(&self, did: String) -> Result<Option<CacheResult>> {
        match self.check_cache_internal(did.clone()).await {
            Ok(res) => Ok(res),
            Err(err) => {
                tracing::error!(%did, ?err, "failed to check did cache");
                Ok(None)
            }
        }
    }

    async fn clear_entry(&self, did: String) -> Result<()> {
        if let Err(err) = self.clear_entry_internal(did.clone()).await {
            tracing::error!(%did, ?err, "clearing did cache entry failed");
        }
        Ok(())
    }

    async fn clear(&self) -> Result<()> {
        self.db
            .run(|conn| {
                conn.execute("DELETE FROM did_doc", [])?;
                Ok(())
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(did: &str) -> DidDocument {
        DidDocument {
            context: None,
            id: did.to_owned(),
            also_known_as: Some(vec![format!("at://{did}.example.com")]),
            verification_method: None,
            service: None,
        }
    }

    async fn cache_with_ttls(
        stale_ttl: Duration,
        max_ttl: Duration,
    ) -> (tempfile::TempDir, DidSqliteCache) {
        let dir = tempfile::tempdir().unwrap();
        let db = get_migrated_db(dir.path().join("did_cache.sqlite"))
            .await
            .unwrap();
        let cache = DidSqliteCache::new(db, BackgroundQueue::default(), stale_ttl, max_ttl);
        (dir, cache)
    }

    #[tokio::test]
    async fn caches_and_returns_fresh_docs() {
        let (_dir, cache) =
            cache_with_ttls(Duration::from_secs(3600), Duration::from_secs(86400)).await;
        assert!(cache
            .check_cache("did:example:alice".to_owned())
            .await
            .unwrap()
            .is_none());

        cache
            .cache_did("did:example:alice".to_owned(), doc("did:example:alice"))
            .await
            .unwrap();
        let result = cache
            .check_cache("did:example:alice".to_owned())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.doc.id, "did:example:alice");
        assert!(!result.stale);
        assert!(!result.expired);

        // caching again updates in place
        let mut updated = doc("did:example:alice");
        updated.also_known_as = None;
        cache
            .cache_did("did:example:alice".to_owned(), updated)
            .await
            .unwrap();
        let result = cache
            .check_cache("did:example:alice".to_owned())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.doc.also_known_as, None);
    }

    #[tokio::test]
    async fn reports_stale_and_expired_entries() {
        let (_dir, stale) =
            cache_with_ttls(Duration::from_millis(0), Duration::from_secs(86400)).await;
        stale
            .cache_did("did:example:bob".to_owned(), doc("did:example:bob"))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        let result = stale
            .check_cache("did:example:bob".to_owned())
            .await
            .unwrap()
            .unwrap();
        assert!(result.stale);
        assert!(!result.expired);

        let (_dir2, expired) =
            cache_with_ttls(Duration::from_millis(0), Duration::from_millis(0)).await;
        expired
            .cache_did("did:example:bob".to_owned(), doc("did:example:bob"))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        let result = expired
            .check_cache("did:example:bob".to_owned())
            .await
            .unwrap()
            .unwrap();
        assert!(result.stale);
        assert!(result.expired);
    }

    #[tokio::test]
    async fn refresh_cache_updates_and_clears_in_background() {
        let (_dir, cache) =
            cache_with_ttls(Duration::from_secs(3600), Duration::from_secs(86400)).await;
        cache
            .refresh_cache(
                "did:example:carol".to_owned(),
                Box::new(|| Box::pin(async { Ok(Some(doc("did:example:carol"))) })),
            )
            .await
            .unwrap();
        cache.process_all().await;
        assert!(cache
            .check_cache("did:example:carol".to_owned())
            .await
            .unwrap()
            .is_some());

        // a doc that no longer resolves clears the entry
        cache
            .refresh_cache(
                "did:example:carol".to_owned(),
                Box::new(|| Box::pin(async { Ok(None) })),
            )
            .await
            .unwrap();
        cache.process_all().await;
        assert!(cache
            .check_cache("did:example:carol".to_owned())
            .await
            .unwrap()
            .is_none());

        // resolution errors are logged and swallowed
        cache
            .refresh_cache(
                "did:example:carol".to_owned(),
                Box::new(|| Box::pin(async { anyhow::bail!("resolution failed") })),
            )
            .await
            .unwrap();
        cache.process_all().await;
        assert!(cache
            .check_cache("did:example:carol".to_owned())
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn clears_entries_and_all() {
        let (_dir, cache) =
            cache_with_ttls(Duration::from_secs(3600), Duration::from_secs(86400)).await;
        cache
            .cache_did("did:example:a".to_owned(), doc("did:example:a"))
            .await
            .unwrap();
        cache
            .cache_did("did:example:b".to_owned(), doc("did:example:b"))
            .await
            .unwrap();
        cache.clear_entry("did:example:a".to_owned()).await.unwrap();
        assert!(cache
            .check_cache("did:example:a".to_owned())
            .await
            .unwrap()
            .is_none());
        assert!(cache
            .check_cache("did:example:b".to_owned())
            .await
            .unwrap()
            .is_some());
        cache.clear().await.unwrap();
        assert!(cache
            .check_cache("did:example:b".to_owned())
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn swallows_cache_errors() {
        let (_dir, cache) =
            cache_with_ttls(Duration::from_secs(3600), Duration::from_secs(86400)).await;
        cache
            .db
            .run(|conn| {
                conn.execute_batch("DROP TABLE did_doc")?;
                Ok(())
            })
            .await
            .unwrap();
        // every cache op degrades gracefully with the table missing
        cache
            .cache_did("did:example:a".to_owned(), doc("did:example:a"))
            .await
            .unwrap();
        assert!(cache
            .check_cache("did:example:a".to_owned())
            .await
            .unwrap()
            .is_none());
        cache.clear_entry("did:example:a".to_owned()).await.unwrap();
        assert!(cache.clear().await.is_err());
    }
}
