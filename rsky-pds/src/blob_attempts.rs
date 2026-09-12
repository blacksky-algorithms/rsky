//! Every physical write to object storage, journaled before it is sent.
//!
//! An object-storage request that times out may still complete later, so
//! whether a namespace is empty can only be known once every attempt against
//! it has a definite outcome. The journal lives outside every actor store
//! and outside the restore set: deleting an account or restoring an older
//! store never erases the evidence, and a namespace with no journal is
//! unprovable rather than empty.

use crate::db::migrator::{migrate_to_latest, Migration, MigrationSet};
use crate::db::sqlite::{Db, Synchronous};
use anyhow::Result;
use rusqlite::{params, OptionalExtension};
use std::path::Path;

const MIGRATIONS: &[Migration] = &[Migration {
    name: "001",
    sql: "CREATE TABLE upload_attempt (\
            id INTEGER PRIMARY KEY AUTOINCREMENT, \
            did TEXT NOT NULL, \
            key TEXT NOT NULL, \
            operation TEXT NOT NULL, \
            \"attemptNo\" INTEGER NOT NULL, \
            \"issuedAt\" TEXT NOT NULL, \
            outcome TEXT, \
            \"resolvedAt\" TEXT\
          );\
          CREATE INDEX upload_attempt_did_key_idx ON upload_attempt (did, key);\
          CREATE INDEX upload_attempt_open_idx ON upload_attempt (did) WHERE outcome IS NULL;\
          CREATE TABLE namespace (\
            did TEXT PRIMARY KEY, \
            \"firstRskyWriteAt\" TEXT NOT NULL, \
            \"tsEraWritesPossible\" INTEGER NOT NULL\
          );",
}];

const MIGRATION_SET: MigrationSet = MigrationSet {
    shared: &[],
    local: MIGRATIONS,
    legacy: None,
};

/// What happened to an attempt, as far as this process saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptOutcome {
    Succeeded,
    Failed(String),
}

impl AttemptOutcome {
    fn as_text(&self) -> String {
        match self {
            AttemptOutcome::Succeeded => "succeeded".to_owned(),
            AttemptOutcome::Failed(reason) => format!("failed: {reason}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub id: i64,
    pub did: String,
    pub key: String,
    pub operation: String,
    pub attempt_no: i64,
    pub issued_at: String,
    /// `None` while the request is in flight, or forever if the process
    /// died before recording an outcome.
    pub outcome: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Namespace {
    pub did: String,
    pub first_rsky_write_at: String,
    /// Whether another implementation could have written objects here that
    /// no attempt row describes.
    pub ts_era_writes_possible: bool,
}

#[derive(Clone)]
pub struct AttemptJournal {
    db: Db,
    coexistence: bool,
}

impl AttemptJournal {
    pub async fn open(location: impl AsRef<Path>, coexistence: bool) -> Result<Self> {
        if let Some(parent) = location
            .as_ref()
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            tokio::fs::create_dir_all(parent).await?;
        }
        let db = Db::open_with(location, Synchronous::Full)?;
        migrate_to_latest(&db, MIGRATION_SET).await?;
        Ok(AttemptJournal { db, coexistence })
    }

    /// Records an attempt before the request is sent and returns its id.
    pub async fn begin(&self, did: &str, key: &str, operation: &str) -> Result<i64> {
        crate::metrics::METRICS.control_journal_write("attempt");
        let (did, key, operation) = (did.to_owned(), key.to_owned(), operation.to_owned());
        let now = rsky_common::now();
        let coexistence = self.coexistence;
        self.db
            .tx(move |tx| {
                tx.execute(
                    "INSERT INTO namespace (did, \"firstRskyWriteAt\", \"tsEraWritesPossible\") \
                     VALUES (?1, ?2, ?3) ON CONFLICT (did) DO NOTHING",
                    params![did, now, coexistence as i64],
                )?;
                let attempt_no: i64 = tx.query_row(
                    "SELECT count(*) + 1 FROM upload_attempt WHERE did = ?1 AND key = ?2",
                    params![did, key],
                    |row| row.get(0),
                )?;
                tx.execute(
                    "INSERT INTO upload_attempt (did, key, operation, \"attemptNo\", \"issuedAt\") \
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![did, key, operation, attempt_no, now],
                )?;
                Ok(tx.last_insert_rowid())
            })
            .await
    }

    pub async fn resolve(&self, id: i64, outcome: AttemptOutcome) -> Result<()> {
        crate::metrics::METRICS.control_journal_write("attempt");
        let now = rsky_common::now();
        let outcome = outcome.as_text();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE upload_attempt SET outcome = ?1, \"resolvedAt\" = ?2 \
                     WHERE id = ?3 AND outcome IS NULL",
                    params![outcome, now, id],
                )?;
                Ok(())
            })
            .await
    }

    /// Attempts with no recorded outcome; while any exists for a namespace
    /// nothing about that namespace can be declared.
    pub async fn unresolved(&self, did: &str) -> Result<Vec<Attempt>> {
        let did = did.to_owned();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, did, key, operation, \"attemptNo\", \"issuedAt\", outcome \
                     FROM upload_attempt WHERE did = ?1 AND outcome IS NULL ORDER BY id",
                )?;
                let rows = stmt
                    .query_map([&did], attempt_from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
    }

    pub async fn attempts(&self, did: &str, key: &str) -> Result<Vec<Attempt>> {
        let (did, key) = (did.to_owned(), key.to_owned());
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, did, key, operation, \"attemptNo\", \"issuedAt\", outcome \
                     FROM upload_attempt WHERE did = ?1 AND key = ?2 ORDER BY id",
                )?;
                let rows = stmt
                    .query_map(params![did, key], attempt_from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
    }

    pub async fn namespace_of(&self, did: &str) -> Result<Option<Namespace>> {
        let did = did.to_owned();
        self.db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT did, \"firstRskyWriteAt\", \"tsEraWritesPossible\" \
                         FROM namespace WHERE did = ?1",
                        [&did],
                        |row| {
                            Ok(Namespace {
                                did: row.get(0)?,
                                first_rsky_write_at: row.get(1)?,
                                ts_era_writes_possible: row.get::<_, i64>(2)? != 0,
                            })
                        },
                    )
                    .optional()?)
            })
            .await
    }
}

fn attempt_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Attempt> {
    Ok(Attempt {
        id: row.get(0)?,
        did: row.get(1)?,
        key: row.get(2)?,
        operation: row.get(3)?,
        attempt_no: row.get(4)?,
        issued_at: row.get(5)?,
        outcome: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn attempts_are_journaled_before_and_resolved_after() {
        let dir = tempfile::tempdir().unwrap();
        let journal = AttemptJournal::open(dir.path().join("rsky/blob-attempts.sqlite"), true)
            .await
            .unwrap();
        assert!(journal.namespace_of("did:plc:a").await.unwrap().is_none());
        let first = journal
            .begin("did:plc:a", "blocks/did:plc:a/bafy1", "put")
            .await
            .unwrap();
        let namespace = journal.namespace_of("did:plc:a").await.unwrap().unwrap();
        assert!(namespace.ts_era_writes_possible);
        assert_eq!(journal.unresolved("did:plc:a").await.unwrap().len(), 1);
        journal
            .resolve(first, AttemptOutcome::Succeeded)
            .await
            .unwrap();
        assert!(journal.unresolved("did:plc:a").await.unwrap().is_empty());

        // a second attempt on the same key is numbered, and its failure kept
        let second = journal
            .begin("did:plc:a", "blocks/did:plc:a/bafy1", "put")
            .await
            .unwrap();
        journal
            .resolve(second, AttemptOutcome::Failed("timeout".to_owned()))
            .await
            .unwrap();
        // resolving twice changes nothing
        journal
            .resolve(second, AttemptOutcome::Succeeded)
            .await
            .unwrap();
        let attempts = journal
            .attempts("did:plc:a", "blocks/did:plc:a/bafy1")
            .await
            .unwrap();
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].attempt_no, 1);
        assert_eq!(attempts[0].outcome.as_deref(), Some("succeeded"));
        assert_eq!(attempts[1].attempt_no, 2);
        assert_eq!(attempts[1].outcome.as_deref(), Some("failed: timeout"));
        assert_eq!(attempts[1].operation, "put");

        // an attempt the process never resolved stays open, and the
        // namespace remembers when this implementation first wrote it
        let open = journal
            .begin("did:plc:a", "tmp/did:plc:a/k", "put")
            .await
            .unwrap();
        assert_eq!(journal.unresolved("did:plc:a").await.unwrap()[0].id, open);
        let again = journal.namespace_of("did:plc:a").await.unwrap().unwrap();
        assert_eq!(again.first_rsky_write_at, namespace.first_rsky_write_at);

        let sole = AttemptJournal::open(dir.path().join("sole.sqlite"), false)
            .await
            .unwrap();
        sole.begin("did:plc:b", "k", "delete").await.unwrap();
        assert!(
            !sole
                .namespace_of("did:plc:b")
                .await
                .unwrap()
                .unwrap()
                .ts_era_writes_possible
        );
    }
}
