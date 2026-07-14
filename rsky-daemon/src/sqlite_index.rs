//! SQLite-backed [`SpaceIndex`], keyed by space. One database serves any
//! number of spaces; the engine sees a per-space handle
//! ([`SqliteIndex::for_space`]) so its signatures stay space-agnostic.

use async_trait::async_trait;
use rsky_space::LtHash;
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::error::{DaemonError, Result};
use crate::index::SpaceIndex;

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
";

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
        Ok(Self {
            conn: Mutex::new(conn),
        })
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
