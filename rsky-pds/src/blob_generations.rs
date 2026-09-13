//! The append-only registry of retired object keys.
//!
//! A delete that timed out may still run later and remove an object that
//! was uploaded again under the same key. Once no other implementation can
//! read the store, the collector retires such a key instead of reusing it:
//! the next upload of the same content lands at a generation key the old
//! delete never named. The registry lives outside every actor store and
//! outside the restore set, so restoring an older store can never rewind a
//! generation, and a collector refuses to run without it.

use crate::db::migrator::{migrate_to_latest, Migration, MigrationSet};
use crate::db::sqlite::{Db, Synchronous};
use anyhow::{bail, Result};
use rusqlite::{params, OptionalExtension};
use std::path::Path;

const MIGRATIONS: &[Migration] = &[Migration {
    name: "001",
    sql: "CREATE TABLE generation (\
            id INTEGER PRIMARY KEY AUTOINCREMENT, \
            did TEXT NOT NULL, \
            cid TEXT NOT NULL, \
            generation INTEGER NOT NULL, \
            \"retiredKey\" TEXT NOT NULL, \
            \"allocatedAt\" TEXT NOT NULL\
          );\
          CREATE INDEX generation_did_cid_idx ON generation (did, cid);",
}];

const MIGRATION_SET: MigrationSet = MigrationSet {
    shared: &[],
    local: MIGRATIONS,
    legacy: None,
};

/// One retirement: the key `retired_key` is never written again for this
/// content, and generation `generation` is the next physical location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Retirement {
    pub did: String,
    pub cid: String,
    pub generation: u32,
    pub retired_key: String,
    pub allocated_at: String,
}

#[derive(Clone)]
pub struct Generations {
    db: Db,
}

/// The physical name of a content's object at a generation: the base name
/// for generation zero, a suffixed name afterwards.
pub fn generation_name(cid: &str, generation: u32) -> String {
    if generation == 0 {
        cid.to_owned()
    } else {
        format!("{cid}.g{generation}")
    }
}

impl Generations {
    /// Opens the registry, creating it. The collector uses `open_existing`
    /// so a missing file is a refusal rather than a fresh, empty history.
    pub async fn open(location: impl AsRef<Path>) -> Result<Self> {
        if let Some(parent) = location
            .as_ref()
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            tokio::fs::create_dir_all(parent).await?;
        }
        let db = Db::open_with(location, Synchronous::Full)?;
        migrate_to_latest(&db, MIGRATION_SET).await?;
        Ok(Generations { db })
    }

    pub async fn open_existing(location: impl AsRef<Path>) -> Result<Self> {
        if !location.as_ref().exists() {
            bail!(
                "the blob generation registry {} does not exist; the collector refuses to run without it",
                location.as_ref().display()
            );
        }
        Self::open(location).await
    }

    /// The generation the content currently lives at; zero until retired.
    pub async fn current(&self, did: &str, cid: &str) -> Result<u32> {
        let (did, cid) = (did.to_owned(), cid.to_owned());
        self.db
            .run(move |conn| {
                let generation: Option<i64> = conn.query_row(
                    "SELECT MAX(generation) FROM generation WHERE did = ?1 AND cid = ?2",
                    params![did, cid],
                    |row| row.get(0),
                )?;
                Ok(u32::try_from(generation.unwrap_or(0)).unwrap_or(u32::MAX))
            })
            .await
    }

    /// Retires `retired_key` for the content and allocates the next
    /// generation, which is returned. Rows are only ever appended.
    pub async fn retire(&self, did: &str, cid: &str, retired_key: &str) -> Result<u32> {
        crate::metrics::METRICS.control_journal_write("generation");
        let (did, cid, retired_key) = (did.to_owned(), cid.to_owned(), retired_key.to_owned());
        let now = rsky_common::now();
        self.db
            .tx(move |tx| {
                let current: Option<i64> = tx.query_row(
                    "SELECT MAX(generation) FROM generation WHERE did = ?1 AND cid = ?2",
                    params![did, cid],
                    |row| row.get(0),
                )?;
                let next = current.unwrap_or(0) + 1;
                tx.execute(
                    "INSERT INTO generation (did, cid, generation, \"retiredKey\", \"allocatedAt\") \
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![did, cid, next, retired_key, now],
                )?;
                Ok(u32::try_from(next).unwrap_or(u32::MAX))
            })
            .await
    }

    pub async fn retirements(&self, did: &str) -> Result<Vec<Retirement>> {
        let did = did.to_owned();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT did, cid, generation, \"retiredKey\", \"allocatedAt\" \
                     FROM generation WHERE did = ?1 ORDER BY id",
                )?;
                let rows = stmt
                    .query_map([&did], |row| {
                        Ok(Retirement {
                            did: row.get(0)?,
                            cid: row.get(1)?,
                            generation: u32::try_from(row.get::<_, i64>(2)?).unwrap_or(u32::MAX),
                            retired_key: row.get(3)?,
                            allocated_at: row.get(4)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
    }

    /// Whether `key` was ever retired for the actor.
    pub async fn is_retired(&self, did: &str, key: &str) -> Result<bool> {
        let (did, key) = (did.to_owned(), key.to_owned());
        self.db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT 1 FROM generation WHERE did = ?1 AND \"retiredKey\" = ?2 LIMIT 1",
                        params![did, key],
                        |_| Ok(()),
                    )
                    .optional()?
                    .is_some())
            })
            .await
    }

    /// When the newest generation was allocated, or nothing when the
    /// registry is empty.
    pub async fn newest_allocation(&self) -> Result<Option<String>> {
        self.db
            .run(|conn| {
                Ok(
                    conn.query_row("SELECT MAX(\"allocatedAt\") FROM generation", [], |row| {
                        row.get::<_, Option<String>>(0)
                    })?,
                )
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn generations_only_ever_advance() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rsky/blob-generations.sqlite");
        assert!(Generations::open_existing(&path).await.is_err());
        assert!(Generations::open("/").await.is_err());
        let registry = Generations::open(&path).await.unwrap();
        assert_eq!(registry.current("did:plc:a", "bafy1").await.unwrap(), 0);
        assert_eq!(registry.newest_allocation().await.unwrap(), None);
        assert_eq!(generation_name("bafy1", 0), "bafy1");
        assert_eq!(generation_name("bafy1", 2), "bafy1.g2");

        let first = registry
            .retire("did:plc:a", "bafy1", "blocks/did:plc:a/bafy1")
            .await
            .unwrap();
        assert_eq!(first, 1);
        let second = registry
            .retire("did:plc:a", "bafy1", "blocks/did:plc:a/bafy1.g1")
            .await
            .unwrap();
        assert_eq!(second, 2);
        assert_eq!(registry.current("did:plc:a", "bafy1").await.unwrap(), 2);
        assert_eq!(registry.current("did:plc:a", "other").await.unwrap(), 0);
        assert!(registry
            .is_retired("did:plc:a", "blocks/did:plc:a/bafy1")
            .await
            .unwrap());
        assert!(!registry
            .is_retired("did:plc:a", "blocks/did:plc:a/bafy1.g2")
            .await
            .unwrap());
        let history = registry.retirements("did:plc:a").await.unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[1].generation, 2);
        assert!(registry.newest_allocation().await.unwrap().is_some());

        // reopening an existing registry keeps every row
        let reopened = Generations::open_existing(&path).await.unwrap();
        assert_eq!(reopened.current("did:plc:a", "bafy1").await.unwrap(), 2);
    }
}
