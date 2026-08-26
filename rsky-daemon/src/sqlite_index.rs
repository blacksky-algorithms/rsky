//! SQLite-backed [`SpaceIndex`], keyed by space. One database serves any
//! number of spaces; the engine sees a per-space handle
//! ([`SqliteIndex::for_space`]) so its signatures stay space-agnostic.

use async_trait::async_trait;
use rsky_space::LtHash;
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::error::{DaemonError, Result};
use crate::index::{IndexMutation, JournaledBatch, SpaceIndex};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS sync_state (
    space_uri    TEXT NOT NULL,
    did          TEXT NOT NULL,
    rev          TEXT NOT NULL,
    lthash_state BLOB NOT NULL,
    PRIMARY KEY (space_uri, did)
);
CREATE TABLE IF NOT EXISTS record (
    space_uri  TEXT NOT NULL,
    did        TEXT NOT NULL,
    collection TEXT NOT NULL,
    rkey       TEXT NOT NULL,
    cid        TEXT NOT NULL,
    rev        TEXT NOT NULL,
    value      BLOB,
    PRIMARY KEY (space_uri, did, collection, rkey)
);
CREATE TABLE IF NOT EXISTS projection_journal (
    space_uri TEXT NOT NULL,
    did       TEXT NOT NULL,
    rev       TEXT NOT NULL,
    mutations BLOB NOT NULL,
    PRIMARY KEY (space_uri, did, rev)
);
CREATE TABLE IF NOT EXISTS projector_cursor (
    projector TEXT NOT NULL,
    space_uri TEXT NOT NULL,
    did       TEXT NOT NULL,
    rev       TEXT NOT NULL,
    PRIMARY KEY (projector, space_uri, did)
);
CREATE TABLE IF NOT EXISTS projection_failure (
    projector     TEXT NOT NULL,
    space_uri     TEXT NOT NULL,
    did           TEXT NOT NULL,
    rev           TEXT NOT NULL,
    attempts      INTEGER NOT NULL,
    last_error    TEXT NOT NULL,
    dead_lettered INTEGER NOT NULL DEFAULT 0,
    denials       INTEGER NOT NULL DEFAULT 0,
    parked        INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (projector, space_uri, did, rev)
);
";

/// Columns added to `projection_failure` after its first release; an index
/// created by an earlier build has the table but not these.
const FAILURE_COLUMNS: [&str; 2] = ["denials", "parked"];

fn db_err(e: rusqlite::Error) -> DaemonError {
    DaemonError::Index(e.to_string())
}

pub struct SqliteIndex {
    conn: Mutex<Connection>,
}

impl SqliteIndex {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path).map_err(db_err)?;
        conn.busy_timeout(Duration::from_secs(5)).map_err(db_err)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(db_err)?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(db_err)?;
        conn.execute_batch(SCHEMA).map_err(db_err)?;
        Self::add_missing_failure_columns(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn add_missing_failure_columns(conn: &Connection) -> Result<()> {
        let mut present = Vec::new();
        {
            let mut stmt = conn
                .prepare("SELECT name FROM pragma_table_info('projection_failure')")
                .map_err(db_err)?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(db_err)?;
            for row in rows {
                present.push(row.map_err(db_err)?);
            }
        }
        for column in FAILURE_COLUMNS {
            if !present.iter().any(|name| name == column) {
                conn.execute(
                    &format!(
                        "ALTER TABLE projection_failure
                         ADD COLUMN {column} INTEGER NOT NULL DEFAULT 0"
                    ),
                    [],
                )
                .map_err(db_err)?;
            }
        }
        Ok(())
    }

    /// A [`SpaceIndex`] handle scoped to one space.
    pub fn for_space(self: &Arc<Self>, space_uri: impl Into<String>) -> SpaceScopedIndex {
        SpaceScopedIndex {
            db: Arc::clone(self),
            space_uri: space_uri.into(),
        }
    }
}

pub struct SpaceScopedIndex {
    db: Arc<SqliteIndex>,
    space_uri: String,
}

#[async_trait]
impl SpaceIndex for SpaceScopedIndex {
    async fn last_rev(&self, did: &str) -> Result<Option<String>> {
        let conn = self.db.conn.lock().unwrap();
        conn.query_row(
            "SELECT rev FROM sync_state WHERE space_uri = ?1 AND did = ?2",
            params![self.space_uri, did],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_err)
    }

    async fn load_lthash(&self, did: &str) -> Result<LtHash> {
        let conn = self.db.conn.lock().unwrap();
        let blob: Option<Vec<u8>> = conn
            .query_row(
                "SELECT lthash_state FROM sync_state WHERE space_uri = ?1 AND did = ?2",
                params![self.space_uri, did],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_err)?;
        match blob {
            Some(bytes) => {
                let state: [u8; 2048] = bytes.try_into().map_err(|b: Vec<u8>| {
                    DaemonError::Index(format!("lthash state has {} bytes, want 2048", b.len()))
                })?;
                Ok(LtHash::from_state_bytes(&state))
            }
            None => Ok(LtHash::new()),
        }
    }

    async fn get_cid(&self, did: &str, collection: &str, rkey: &str) -> Result<Option<String>> {
        let conn = self.db.conn.lock().unwrap();
        conn.query_row(
            "SELECT cid FROM record
             WHERE space_uri = ?1 AND did = ?2 AND collection = ?3 AND rkey = ?4",
            params![self.space_uri, did, collection, rkey],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_err)
    }

    async fn upsert(
        &self,
        did: &str,
        collection: &str,
        rkey: &str,
        cid: &str,
        rev: &str,
        value: Option<Vec<u8>>,
    ) -> Result<()> {
        let conn = self.db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO record (space_uri, did, collection, rkey, cid, rev, value)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (space_uri, did, collection, rkey)
             DO UPDATE SET cid = ?5, rev = ?6, value = ?7",
            params![self.space_uri, did, collection, rkey, cid, rev, value],
        )
        .map_err(db_err)?;
        Ok(())
    }

    async fn delete(&self, did: &str, collection: &str, rkey: &str) -> Result<()> {
        let conn = self.db.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM record
             WHERE space_uri = ?1 AND did = ?2 AND collection = ?3 AND rkey = ?4",
            params![self.space_uri, did, collection, rkey],
        )
        .map_err(db_err)?;
        Ok(())
    }

    async fn save_head(&self, did: &str, rev: &str, lthash: &LtHash) -> Result<()> {
        let conn = self.db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO sync_state (space_uri, did, rev, lthash_state)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (space_uri, did) DO UPDATE SET rev = ?3, lthash_state = ?4",
            params![self.space_uri, did, rev, lthash.state_bytes().to_vec()],
        )
        .map_err(db_err)?;
        Ok(())
    }

    async fn journal_batch(&self, did: &str, rev: &str, mutations: &[IndexMutation]) -> Result<()> {
        let encoded = serde_json::to_vec(mutations)
            .map_err(|error| DaemonError::Index(format!("journal encode: {error}")))?;
        let conn = self.db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO projection_journal (space_uri, did, rev, mutations)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (space_uri, did, rev) DO NOTHING",
            params![self.space_uri, did, rev, encoded],
        )
        .map_err(db_err)?;
        Ok(())
    }

    async fn pending_batches(&self, projector: &str) -> Result<Vec<JournaledBatch>> {
        let conn = self.db.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT j.did, j.rev, j.mutations, COALESCE(f.denials, 0)
                 FROM projection_journal j
                 LEFT JOIN projector_cursor c
                   ON c.projector = ?1 AND c.space_uri = j.space_uri AND c.did = j.did
                 LEFT JOIN projection_failure f
                   ON f.projector = ?1 AND f.space_uri = j.space_uri AND f.did = j.did AND f.rev = j.rev
                 WHERE j.space_uri = ?2
                   AND (c.rev IS NULL OR j.rev > c.rev)
                   AND COALESCE(f.dead_lettered, 0) = 0
                   AND COALESCE(f.parked, 0) = 0
                 ORDER BY j.did, j.rev",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![projector, self.space_uri], |row| {
                let mutations: Vec<u8> = row.get(2)?;
                let mutations = serde_json::from_slice(&mutations)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;
                Ok(JournaledBatch {
                    author: row.get(0)?,
                    rev: row.get(1)?,
                    mutations,
                    denials: row.get(3)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db_err)
    }

    async fn advance_projector_cursor(&self, projector: &str, did: &str, rev: &str) -> Result<()> {
        let conn = self.db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO projector_cursor (projector, space_uri, did, rev) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (projector, space_uri, did) DO UPDATE SET rev = ?4",
            params![projector, self.space_uri, did, rev],
        )
        .map_err(db_err)?;
        Ok(())
    }

    async fn record_projection_failure(
        &self,
        projector: &str,
        did: &str,
        rev: &str,
        error: &str,
        dead_letter_after: u32,
    ) -> Result<u32> {
        let conn = self.db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO projection_failure
                 (projector, space_uri, did, rev, attempts, last_error, dead_lettered)
             VALUES (?1, ?2, ?3, ?4, 1, ?5, CASE WHEN 1 >= ?6 THEN 1 ELSE 0 END)
             ON CONFLICT (projector, space_uri, did, rev) DO UPDATE
                 SET attempts = attempts + 1,
                     last_error = ?5,
                     dead_lettered = CASE WHEN attempts + 1 >= ?6 THEN 1 ELSE 0 END",
            params![
                projector,
                self.space_uri,
                did,
                rev,
                error,
                dead_letter_after
            ],
        )
        .map_err(db_err)?;
        conn.query_row(
            "SELECT attempts FROM projection_failure
             WHERE projector = ?1 AND space_uri = ?2 AND did = ?3 AND rev = ?4",
            params![projector, self.space_uri, did, rev],
            |row| row.get(0),
        )
        .map_err(db_err)
    }

    async fn record_projection_denial(
        &self,
        projector: &str,
        did: &str,
        rev: &str,
        error: &str,
        park_after: u32,
    ) -> Result<u32> {
        let conn = self.db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO projection_failure
                 (projector, space_uri, did, rev, attempts, last_error, dead_lettered,
                  denials, parked)
             VALUES (?1, ?2, ?3, ?4, 0, ?5, 0, 1, CASE WHEN 1 >= ?6 THEN 1 ELSE 0 END)
             ON CONFLICT (projector, space_uri, did, rev) DO UPDATE
                 SET denials = denials + 1,
                     last_error = ?5,
                     parked = CASE WHEN denials + 1 >= ?6 THEN 1 ELSE 0 END",
            params![projector, self.space_uri, did, rev, error, park_after],
        )
        .map_err(db_err)?;
        conn.query_row(
            "SELECT denials FROM projection_failure
             WHERE projector = ?1 AND space_uri = ?2 AND did = ?3 AND rev = ?4",
            params![projector, self.space_uri, did, rev],
            |row| row.get(0),
        )
        .map_err(db_err)
    }

    async fn prune_journal(&self, projectors: &[&str]) -> Result<usize> {
        if projectors.is_empty() {
            return Ok(0);
        }
        let mut conn = self.db.conn.lock().unwrap();
        let tx = conn.transaction().map_err(db_err)?;
        let placeholders = (0..projectors.len())
            .map(|i| format!("?{}", i + 3))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "DELETE FROM projection_journal WHERE space_uri = ?1
             AND (SELECT COUNT(*) FROM projector_cursor c
                  WHERE c.space_uri = projection_journal.space_uri
                    AND c.did = projection_journal.did
                    AND c.rev >= projection_journal.rev
                    AND c.projector IN ({placeholders})) = ?2
             AND NOT EXISTS (SELECT 1 FROM projection_failure f
                  WHERE f.space_uri = projection_journal.space_uri
                    AND f.did = projection_journal.did
                    AND f.rev = projection_journal.rev
                    AND (f.dead_lettered = 1 OR f.parked = 1))"
        );
        let count = projectors.len() as i64;
        let mut values: Vec<&dyn rusqlite::ToSql> = vec![&self.space_uri, &count];
        for projector in projectors {
            values.push(projector);
        }
        let pruned = tx.execute(&sql, &values[..]).map_err(db_err)?;
        tx.execute(
            "DELETE FROM projection_failure
             WHERE space_uri = ?1 AND dead_lettered = 0 AND parked = 0
             AND NOT EXISTS (SELECT 1 FROM projection_journal j
                  WHERE j.space_uri = projection_failure.space_uri
                    AND j.did = projection_failure.did
                    AND j.rev = projection_failure.rev)",
            params![self.space_uri],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(pruned)
    }

    async fn list_paths(&self, did: &str) -> Result<Vec<(String, String, String)>> {
        let conn = self.db.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT collection, rkey, cid FROM record
                 WHERE space_uri = ?1 AND did = ?2",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![self.space_uri, did], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(db_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db_err)
    }

    async fn purge_space(&self) -> Result<()> {
        let conn = self.db.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM record WHERE space_uri = ?1",
            params![self.space_uri],
        )
        .map_err(db_err)?;
        conn.execute(
            "DELETE FROM sync_state WHERE space_uri = ?1",
            params![self.space_uri],
        )
        .map_err(db_err)?;
        for table in [
            "projection_journal",
            "projector_cursor",
            "projection_failure",
        ] {
            conn.execute(
                &format!("DELETE FROM {table} WHERE space_uri = ?1"),
                params![self.space_uri],
            )
            .map_err(db_err)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::sync_repo;
    use crate::recovery::recover_repo;
    use crate::recovery::tests::{
        author, car_bytes, fixture, signed_commit_for, CarHost, FixedKey, AUTHOR, SPACE,
    };

    fn open_at(dir: &tempfile::TempDir) -> Arc<SqliteIndex> {
        let path = dir.path().join("index.sqlite");
        Arc::new(SqliteIndex::open(path.to_str().unwrap()).unwrap())
    }

    #[tokio::test]
    async fn record_and_head_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_at(&dir);
        let index = db.for_space(SPACE);

        assert_eq!(index.last_rev(AUTHOR).await.unwrap(), None);
        assert_eq!(index.get_cid(AUTHOR, "c.o.l", "3ka").await.unwrap(), None);
        assert!(index.list_paths(AUTHOR).await.unwrap().is_empty());
        let empty = index.load_lthash(AUTHOR).await.unwrap();
        assert_eq!(empty.hash(), LtHash::new().hash());

        index
            .upsert(AUTHOR, "c.o.l", "3ka", "bafyA", "3rev", Some(vec![1, 2]))
            .await
            .unwrap();
        index
            .upsert(AUTHOR, "c.o.l", "3ka", "bafyB", "3rev2", None)
            .await
            .unwrap();
        assert_eq!(
            index.get_cid(AUTHOR, "c.o.l", "3ka").await.unwrap(),
            Some("bafyB".to_string())
        );
        assert_eq!(
            index.list_paths(AUTHOR).await.unwrap(),
            vec![("c.o.l".to_string(), "3ka".to_string(), "bafyB".to_string())]
        );

        let mut lth = LtHash::new();
        lth.add("c.o.l/3ka/bafyB");
        index.save_head(AUTHOR, "3rev2", &lth).await.unwrap();
        index.save_head(AUTHOR, "3rev3", &lth).await.unwrap();
        assert_eq!(
            index.last_rev(AUTHOR).await.unwrap(),
            Some("3rev3".to_string())
        );
        assert_eq!(index.load_lthash(AUTHOR).await.unwrap().hash(), lth.hash());

        index.delete(AUTHOR, "c.o.l", "3ka").await.unwrap();
        assert_eq!(index.get_cid(AUTHOR, "c.o.l", "3ka").await.unwrap(), None);
    }

    #[tokio::test]
    async fn full_engine_recovery_run_and_reopen_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let f = fixture();
        let commit = signed_commit_for(&f.author, &f.entries, "3rev");
        let host = CarHost(car_bytes(&f, &commit).await);
        let keys = FixedKey(f.author.did_key.clone());

        {
            let db = open_at(&dir);
            let index = db.for_space(SPACE);
            let outcome = recover_repo(&host, &index, &keys, SPACE, AUTHOR)
                .await
                .unwrap();
            assert!(outcome.commit_verified);
            assert_eq!(outcome.ops_applied, 3);
        }

        // Reopen: the head and records persisted, so a subsequent incremental
        // sync starting from the stored state verifies the same commit.
        let db = open_at(&dir);
        let index = db.for_space(SPACE);
        assert_eq!(
            index.last_rev(AUTHOR).await.unwrap(),
            Some("3rev".to_string())
        );
        assert_eq!(index.list_paths(AUTHOR).await.unwrap().len(), 3);
        assert_eq!(
            index.load_lthash(AUTHOR).await.unwrap().hash().to_vec(),
            commit.hash.to_vec()
        );
    }

    #[tokio::test]
    async fn engine_sync_repo_runs_against_sqlite() {
        use crate::repohost::{OplogPage, RepoHostClient};
        use rsky_space::types::RepoOp;
        use serde_bytes::ByteBuf;
        use std::collections::BTreeMap;

        struct OneOpHost(OplogPage);
        #[async_trait]
        impl RepoHostClient for OneOpHost {
            async fn list_repo_ops(
                &self,
                _space: &str,
                _did: &str,
                _since: Option<&str>,
                _cursor: Option<&str>,
            ) -> Result<OplogPage> {
                Ok(OplogPage {
                    ops: self.0.ops.clone(),
                    commit: self.0.commit.clone(),
                    cursor: None,
                })
            }
            async fn get_repo_car(&self, _space: &str, _did: &str) -> Result<Vec<u8>> {
                Err(DaemonError::Xrpc("unused".to_string()))
            }
            async fn get_latest_commit(
                &self,
                _space: &str,
                _did: &str,
            ) -> Result<rsky_space::types::SignedCommit> {
                Err(DaemonError::Xrpc("unused".to_string()))
            }
        }

        let a = author();
        let (cid, _) = crate::recovery::tests::raw_block("post one");
        let mut entries = BTreeMap::new();
        entries.insert(format!("community.blacksky.feed.post/{}", "3ka"), cid);
        let commit = signed_commit_for(&a, &entries, "3rev");
        let host = OneOpHost(OplogPage {
            ops: vec![RepoOp {
                rev: "3rev".to_string(),
                collection: "community.blacksky.feed.post".to_string(),
                rkey: "3ka".to_string(),
                cid: Some(cid.to_string()),
                prev: None,
                value: Some(ByteBuf::from(b"post one".to_vec())),
            }],
            commit: Some(commit),
            cursor: None,
        });

        let dir = tempfile::tempdir().unwrap();
        let db = open_at(&dir);
        let index = db.for_space(SPACE);
        let keys = FixedKey(a.did_key.clone());
        let outcome = sync_repo(&host, &index, &keys, SPACE, AUTHOR)
            .await
            .unwrap();
        assert!(outcome.commit_verified);
        assert_eq!(outcome.ops_applied, 1);
        assert_eq!(
            index
                .get_cid(AUTHOR, "community.blacksky.feed.post", "3ka")
                .await
                .unwrap(),
            Some(cid.to_string())
        );

        assert!(host.get_repo_car(SPACE, AUTHOR).await.is_err());
        assert!(host.get_latest_commit(SPACE, AUTHOR).await.is_err());
    }

    #[tokio::test]
    async fn purge_space_only_clears_its_own_space() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_at(&dir);
        let one = db.for_space(SPACE);
        let other = db.for_space("at://did:plc:other/space/t/main");

        for index in [&one, &other] {
            index
                .upsert(AUTHOR, "c.o.l", "3ka", "bafyA", "3rev", None)
                .await
                .unwrap();
            index
                .save_head(AUTHOR, "3rev", &LtHash::new())
                .await
                .unwrap();
        }

        one.purge_space().await.unwrap();
        assert_eq!(one.last_rev(AUTHOR).await.unwrap(), None);
        assert!(one.list_paths(AUTHOR).await.unwrap().is_empty());
        assert_eq!(
            other.last_rev(AUTHOR).await.unwrap(),
            Some("3rev".to_string())
        );
        assert_eq!(other.list_paths(AUTHOR).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn projector_cursors_are_independent_and_survive_dead_letters() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_at(&dir);
        let index = db.for_space(SPACE);
        let mutation = |rkey: &str| IndexMutation::Delete {
            collection: "app.bsky.feed.post".to_string(),
            rkey: rkey.to_string(),
        };

        index
            .journal_batch(AUTHOR, "3rev1", &[mutation("3ka")])
            .await
            .unwrap();
        index
            .journal_batch(AUTHOR, "3rev2", &[mutation("3kb")])
            .await
            .unwrap();
        // A replayed batch must not duplicate its journal row.
        index
            .journal_batch(AUTHOR, "3rev1", &[mutation("3ka")])
            .await
            .unwrap();
        assert_eq!(index.pending_batches("feeds").await.unwrap().len(), 2);

        index
            .advance_projector_cursor("feeds", AUTHOR, "3rev1")
            .await
            .unwrap();
        let pending = index.pending_batches("feeds").await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].rev, "3rev2");
        assert_eq!(pending[0].mutations, vec![mutation("3kb")]);
        assert_eq!(index.pending_batches("appview").await.unwrap().len(), 2);

        assert_eq!(
            index
                .record_projection_failure("feeds", AUTHOR, "3rev2", "boom", 2)
                .await
                .unwrap(),
            1
        );
        assert_eq!(index.pending_batches("feeds").await.unwrap().len(), 1);
        assert_eq!(
            index
                .record_projection_failure("feeds", AUTHOR, "3rev2", "boom", 2)
                .await
                .unwrap(),
            2
        );
        assert!(index.pending_batches("feeds").await.unwrap().is_empty());

        // Nothing prunes while a dead-lettered batch is still on the journal.
        index
            .advance_projector_cursor("appview", AUTHOR, "3rev2")
            .await
            .unwrap();
        index
            .advance_projector_cursor("feeds", AUTHOR, "3rev2")
            .await
            .unwrap();
        assert_eq!(index.prune_journal(&["feeds", "appview"]).await.unwrap(), 1);
        let remaining: i64 = {
            let conn = db.conn.lock().unwrap();
            conn.query_row(
                "SELECT COUNT(*) FROM projection_journal WHERE space_uri = ?1",
                params![SPACE],
                |row| row.get(0),
            )
            .unwrap()
        };
        assert_eq!(remaining, 1);

        index.purge_space().await.unwrap();
        assert!(index.pending_batches("appview").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn denials_are_budgeted_apart_from_dead_letters_and_survive_a_prune() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_at(&dir);
        let index = db.for_space(SPACE);
        let mutation = IndexMutation::Delete {
            collection: "app.bsky.feed.post".to_string(),
            rkey: "3ka".to_string(),
        };
        index
            .journal_batch(AUTHOR, "3rev1", &[mutation])
            .await
            .unwrap();

        assert_eq!(
            index
                .record_projection_denial("appview", AUTHOR, "3rev1", "401", 2)
                .await
                .unwrap(),
            1
        );
        let pending = index.pending_batches("appview").await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].denials, 1);

        // A denial leaves the poison budget untouched.
        let attempts: u32 = {
            let conn = db.conn.lock().unwrap();
            conn.query_row(
                "SELECT attempts FROM projection_failure
                 WHERE projector = 'appview' AND space_uri = ?1 AND did = ?2 AND rev = '3rev1'",
                params![SPACE, AUTHOR],
                |row| row.get(0),
            )
            .unwrap()
        };
        assert_eq!(attempts, 0);

        assert_eq!(
            index
                .record_projection_denial("appview", AUTHOR, "3rev1", "401", 2)
                .await
                .unwrap(),
            2
        );
        assert!(index.pending_batches("appview").await.unwrap().is_empty());

        // A parked batch keeps its journal row, so it stays inspectable.
        index
            .advance_projector_cursor("appview", AUTHOR, "3rev1")
            .await
            .unwrap();
        assert_eq!(index.prune_journal(&["appview"]).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn an_index_written_before_the_denial_columns_is_migrated_on_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.sqlite");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE projection_failure (
                     projector     TEXT NOT NULL,
                     space_uri     TEXT NOT NULL,
                     did           TEXT NOT NULL,
                     rev           TEXT NOT NULL,
                     attempts      INTEGER NOT NULL,
                     last_error    TEXT NOT NULL,
                     dead_lettered INTEGER NOT NULL DEFAULT 0,
                     PRIMARY KEY (projector, space_uri, did, rev)
                 );",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO projection_failure
                     (projector, space_uri, did, rev, attempts, last_error, dead_lettered)
                 VALUES ('appview', ?1, ?2, '3rev1', 3, 'boom', 1)",
                params![SPACE, AUTHOR],
            )
            .unwrap();
        }

        let db = Arc::new(SqliteIndex::open(path.to_str().unwrap()).unwrap());
        let index = db.for_space(SPACE);
        assert_eq!(
            index
                .record_projection_denial("appview", AUTHOR, "3rev2", "401", 5)
                .await
                .unwrap(),
            1
        );
        let (denials, parked, dead): (u32, u32, u32) = {
            let conn = db.conn.lock().unwrap();
            conn.query_row(
                "SELECT denials, parked, dead_lettered FROM projection_failure
                 WHERE rev = '3rev1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap()
        };
        assert_eq!((denials, parked, dead), (0, 0, 1));
    }

    #[tokio::test]
    async fn corrupt_lthash_state_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_at(&dir);
        let index = db.for_space(SPACE);
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO sync_state (space_uri, did, rev, lthash_state)
                 VALUES (?1, ?2, ?3, ?4)",
                params![SPACE, AUTHOR, "3rev", vec![0u8; 7]],
            )
            .unwrap();
        }
        let err = index.load_lthash(AUTHOR).await.err().unwrap();
        assert!(matches!(err, DaemonError::Index(m) if m.contains("7 bytes")));
    }

    #[test]
    fn unopenable_path_is_an_error() {
        let err = SqliteIndex::open("/nonexistent-dir/db.sqlite")
            .err()
            .unwrap();
        assert!(matches!(err, DaemonError::Index(_)));
    }
}
