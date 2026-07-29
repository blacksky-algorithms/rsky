//! Host state: the writer set (spec §The sync boundary) and notification
//! registrations (spec §Write notifications), with in-memory and SQLite
//! backings. Volume is low (one row per writer / per syncer endpoint), so the
//! SQLite connection sits behind a plain mutex.

use async_trait::async_trait;
use rsky_lexicon::com::atproto::space::RepoRef;
use rusqlite::Connection;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use crate::attestation::{JtiStore, JTI_PURGE_GRACE_SECS};
use crate::error::{HostError, Result};

/// The repos known to hold data in a space, with each repo's latest `rev`/`hash`.
#[async_trait]
pub trait WriterSetStore: Send + Sync {
    async fn upsert_writer(
        &self,
        space_uri: &str,
        did: &str,
        rev: &str,
        hash: Option<&str>,
        updated_at: u64,
    ) -> Result<()>;
    /// Page through writers ordered by DID; `cursor` is the last DID of the
    /// previous page. Returns the page and the next cursor.
    async fn list_writers(
        &self,
        space_uri: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<(Vec<RepoRef>, Option<String>)>;
}

/// Endpoints registered (via `registerNotify`) to receive write notifications.
#[async_trait]
pub trait RegistrationStore: Send + Sync {
    async fn register(&self, space_uri: &str, endpoint: &str, expires_at: u64) -> Result<()>;
    async fn endpoints(&self, space_uri: &str, now: u64) -> Result<Vec<String>>;
}

fn next_cursor(page: &[RepoRef], limit: u32) -> Option<String> {
    if page.len() == limit as usize {
        page.last().map(|r| r.did.clone())
    } else {
        None
    }
}

/// Keyed by `(space_uri, did)`, holding the writer's latest `(rev, hash)`.
type WriterMap = BTreeMap<(String, String), (String, Option<String>)>;

#[derive(Default)]
pub struct InMemoryWriterSet {
    writers: Mutex<WriterMap>,
}

#[async_trait]
impl WriterSetStore for InMemoryWriterSet {
    async fn upsert_writer(
        &self,
        space_uri: &str,
        did: &str,
        rev: &str,
        hash: Option<&str>,
        _updated_at: u64,
    ) -> Result<()> {
        self.writers.lock().unwrap().insert(
            (space_uri.to_string(), did.to_string()),
            (rev.to_string(), hash.map(str::to_string)),
        );
        Ok(())
    }

    async fn list_writers(
        &self,
        space_uri: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<(Vec<RepoRef>, Option<String>)> {
        let writers = self.writers.lock().unwrap();
        let page: Vec<RepoRef> = writers
            .iter()
            .filter(|((space, did), _)| {
                space == space_uri && cursor.is_none_or(|c| did.as_str() > c)
            })
            .take(limit as usize)
            .map(|((_, did), (rev, hash))| RepoRef {
                did: did.clone(),
                rev: rev.clone(),
                hash: hash.clone(),
            })
            .collect();
        let cursor = next_cursor(&page, limit);
        Ok((page, cursor))
    }
}

#[derive(Default)]
pub struct InMemoryRegistrations {
    endpoints: Mutex<BTreeMap<(String, String), u64>>,
}

#[async_trait]
impl RegistrationStore for InMemoryRegistrations {
    async fn register(&self, space_uri: &str, endpoint: &str, expires_at: u64) -> Result<()> {
        self.endpoints
            .lock()
            .unwrap()
            .insert((space_uri.to_string(), endpoint.to_string()), expires_at);
        Ok(())
    }

    async fn endpoints(&self, space_uri: &str, now: u64) -> Result<Vec<String>> {
        Ok(self
            .endpoints
            .lock()
            .unwrap()
            .iter()
            .filter(|((space, _), expires_at)| space == space_uri && **expires_at > now)
            .map(|((_, endpoint), _)| endpoint.clone())
            .collect())
    }
}

/// SQLite-backed host state (`writer`, `registration`, and `used_jti` tables).
pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::init(Connection::open(path).map_err(sql_err)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory().map_err(sql_err)?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS writer (
                space_uri TEXT NOT NULL,
                did TEXT NOT NULL,
                rev TEXT NOT NULL,
                hash TEXT,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (space_uri, did)
            );
            CREATE TABLE IF NOT EXISTS registration (
                space_uri TEXT NOT NULL,
                endpoint TEXT NOT NULL,
                expires_at INTEGER NOT NULL,
                PRIMARY KEY (space_uri, endpoint)
            );
            CREATE TABLE IF NOT EXISTS used_jti (
                jti TEXT PRIMARY KEY,
                exp INTEGER NOT NULL
            );",
        )
        .map_err(sql_err)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

fn sql_err(e: rusqlite::Error) -> HostError {
    HostError::Store(e.to_string())
}

#[async_trait]
impl WriterSetStore for SqliteStore {
    async fn upsert_writer(
        &self,
        space_uri: &str,
        did: &str,
        rev: &str,
        hash: Option<&str>,
        updated_at: u64,
    ) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO writer (space_uri, did, rev, hash, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (space_uri, did)
                 DO UPDATE SET rev = ?3, hash = ?4, updated_at = ?5",
                rusqlite::params![space_uri, did, rev, hash, updated_at],
            )
            .map_err(sql_err)?;
        Ok(())
    }

    async fn list_writers(
        &self,
        space_uri: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<(Vec<RepoRef>, Option<String>)> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT did, rev, hash FROM writer
                 WHERE space_uri = ?1 AND did > ?2
                 ORDER BY did ASC LIMIT ?3",
            )
            .map_err(sql_err)?;
        let rows = stmt
            .query_map(
                rusqlite::params![space_uri, cursor.unwrap_or(""), limit],
                |row| {
                    Ok(RepoRef {
                        did: row.get(0)?,
                        rev: row.get(1)?,
                        hash: row.get(2)?,
                    })
                },
            )
            .map_err(sql_err)?;
        let page = rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sql_err)?;
        let cursor = next_cursor(&page, limit);
        Ok((page, cursor))
    }
}

#[async_trait]
impl RegistrationStore for SqliteStore {
    async fn register(&self, space_uri: &str, endpoint: &str, expires_at: u64) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO registration (space_uri, endpoint, expires_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT (space_uri, endpoint) DO UPDATE SET expires_at = ?3",
                rusqlite::params![space_uri, endpoint, expires_at],
            )
            .map_err(sql_err)?;
        Ok(())
    }

    async fn endpoints(&self, space_uri: &str, now: u64) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT endpoint FROM registration
                 WHERE space_uri = ?1 AND expires_at > ?2 ORDER BY endpoint ASC",
            )
            .map_err(sql_err)?;
        let rows = stmt
            .query_map(rusqlite::params![space_uri, now], |row| row.get(0))
            .map_err(sql_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sql_err)
    }
}

#[async_trait]
impl JtiStore for SqliteStore {
    async fn consume(&self, jti: &str, exp: u64) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM used_jti WHERE exp + ?1 <= ?2",
            rusqlite::params![JTI_PURGE_GRACE_SECS, exp],
        )
        .map_err(sql_err)?;
        let inserted = conn
            .execute(
                "INSERT OR IGNORE INTO used_jti (jti, exp) VALUES (?1, ?2)",
                rusqlite::params![jti, exp],
            )
            .map_err(sql_err)?;
        Ok(inserted == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPACE: &str = "at://did:plc:auth/space/community.blacksky.feed/main";
    const OTHER_SPACE: &str = "at://did:plc:auth/space/community.blacksky.feed/other";

    async fn exercise_writer_set(store: &dyn WriterSetStore) {
        store
            .upsert_writer(SPACE, "did:plc:bbb", "rev1", None, 10)
            .await
            .unwrap();
        store
            .upsert_writer(SPACE, "did:plc:aaa", "rev1", Some("hash-a"), 10)
            .await
            .unwrap();
        store
            .upsert_writer(SPACE, "did:plc:ccc", "rev1", None, 10)
            .await
            .unwrap();
        store
            .upsert_writer(OTHER_SPACE, "did:plc:zzz", "rev1", None, 10)
            .await
            .unwrap();
        // Upsert replaces rev/hash for an existing writer.
        store
            .upsert_writer(SPACE, "did:plc:bbb", "rev2", Some("hash-b"), 20)
            .await
            .unwrap();

        let (page, cursor) = store.list_writers(SPACE, None, 2).await.unwrap();
        assert_eq!(
            page.iter().map(|r| r.did.as_str()).collect::<Vec<_>>(),
            vec!["did:plc:aaa", "did:plc:bbb"]
        );
        assert_eq!(page[0].hash.as_deref(), Some("hash-a"));
        assert_eq!(page[1].rev, "rev2");
        assert_eq!(page[1].hash.as_deref(), Some("hash-b"));
        assert_eq!(cursor.as_deref(), Some("did:plc:bbb"));

        let (page, cursor) = store
            .list_writers(SPACE, cursor.as_deref(), 2)
            .await
            .unwrap();
        assert_eq!(
            page.iter().map(|r| r.did.as_str()).collect::<Vec<_>>(),
            vec!["did:plc:ccc"]
        );
        assert!(cursor.is_none());

        let (page, _) = store.list_writers(OTHER_SPACE, None, 10).await.unwrap();
        assert_eq!(page.len(), 1);
    }

    async fn exercise_registrations(store: &dyn RegistrationStore) {
        store
            .register(SPACE, "https://syncer.example/a", 100)
            .await
            .unwrap();
        store
            .register(SPACE, "https://syncer.example/b", 50)
            .await
            .unwrap();
        store
            .register(OTHER_SPACE, "https://syncer.example/c", 100)
            .await
            .unwrap();
        // Re-registration extends the expiry.
        store
            .register(SPACE, "https://syncer.example/b", 200)
            .await
            .unwrap();

        let live = store.endpoints(SPACE, 99).await.unwrap();
        assert_eq!(
            live,
            vec![
                "https://syncer.example/a".to_string(),
                "https://syncer.example/b".to_string()
            ]
        );
        let later = store.endpoints(SPACE, 150).await.unwrap();
        assert_eq!(later, vec!["https://syncer.example/b".to_string()]);
        assert!(store.endpoints(SPACE, 500).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn in_memory_writer_set_and_registrations() {
        exercise_writer_set(&InMemoryWriterSet::default()).await;
        exercise_registrations(&InMemoryRegistrations::default()).await;
    }

    #[tokio::test]
    async fn sqlite_writer_set_registrations_and_jti() {
        let store = SqliteStore::open_in_memory().unwrap();
        exercise_writer_set(&store).await;
        exercise_registrations(&store).await;

        assert!(store.consume("jti-1", 100).await.unwrap());
        assert!(!store.consume("jti-1", 100).await.unwrap());
        // A much later token purges the long-expired entry.
        assert!(store
            .consume("jti-2", 100 + JTI_PURGE_GRACE_SECS)
            .await
            .unwrap());
        assert!(store
            .consume("jti-1", 100 + JTI_PURGE_GRACE_SECS)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn sqlite_persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("space_host.db");
        {
            let store = SqliteStore::open(&path).unwrap();
            store
                .upsert_writer(SPACE, "did:plc:aaa", "rev1", None, 10)
                .await
                .unwrap();
        }
        let store = SqliteStore::open(&path).unwrap();
        let (page, _) = store.list_writers(SPACE, None, 10).await.unwrap();
        assert_eq!(page.len(), 1);

        // Unopenable paths surface as store errors.
        assert!(matches!(
            SqliteStore::open(dir.path().join("missing/nested.db")),
            Err(HostError::Store(_))
        ));
    }
}
