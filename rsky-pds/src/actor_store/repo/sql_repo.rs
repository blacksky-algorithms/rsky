use crate::actor_store::db::{ActorDb, RepoBlock};
use anyhow::Result;
use futures::Stream;
use lexicon_cid::Cid;
use rsky_common;
use rsky_repo::block_map::{BlockMap, BlocksAndMissing};
use rsky_repo::cid_set::CidSet;
use rsky_repo::storage::readable_blockstore::ReadableBlockstore;
use rsky_repo::storage::types::RepoStorage;
use rsky_repo::storage::CidAndRev;
use rsky_repo::storage::RepoRootError::RepoRootNotFoundError;
use rsky_repo::types::CommitData;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

pub(crate) fn placeholders(len: usize) -> String {
    vec!["?"; len].join(",")
}

/// The repository root moved between formatting a commit and applying it.
#[derive(Debug, thiserror::Error, PartialEq)]
#[error("ConcurrentWriteError: repo root changed while the commit was prepared")]
pub struct ConcurrentWriteError;

/// An export is one read snapshot; one that outlives this bound is abandoned
/// rather than pinning the WAL indefinitely.
pub const EXPORT_DEADLINE: Duration = Duration::from_secs(600);

const EXPORT_PAGE: usize = 500;

fn put_many_in(conn: &Connection, blocks: &BlockMap, rev: &str) -> Result<()> {
    let mut stmt = conn.prepare_cached(
        "INSERT INTO repo_block (cid, \"repoRev\", size, content) \
         VALUES (?1, ?2, ?3, ?4) ON CONFLICT DO NOTHING",
    )?;
    for (cid, bytes) in blocks.map.iter() {
        stmt.execute(rusqlite::params![
            cid.to_string(),
            rev,
            bytes.0.len() as i64,
            bytes.0
        ])?;
    }
    Ok(())
}

fn delete_many_in(conn: &Connection, cids: &[Cid]) -> Result<()> {
    let cid_strings: Vec<String> = cids.iter().map(|c| c.to_string()).collect();
    for batch in cid_strings.chunks(500) {
        let sql = format!(
            "DELETE FROM repo_block WHERE cid IN ({})",
            placeholders(batch.len())
        );
        conn.execute(&sql, rusqlite::params_from_iter(batch.iter()))?;
    }
    Ok(())
}

/// Applies a commit on a connection that is already inside a transaction.
/// For an existing repository the root is replaced only if it still equals
/// `expected_root`, so a commit prepared against a stale root cannot land.
pub fn apply_commit_in(
    conn: &Connection,
    did: &str,
    now: &str,
    commit: &CommitData,
    expected_root: Option<&Cid>,
) -> Result<()> {
    match expected_root {
        None => {
            conn.execute(
                "INSERT INTO repo_root (did, cid, rev, \"indexedAt\") VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![did, commit.cid.to_string(), commit.rev, now],
            )?;
        }
        Some(expected) => {
            let changed = conn.execute(
                "UPDATE repo_root SET cid = ?1, rev = ?2, \"indexedAt\" = ?3 WHERE cid = ?4",
                rusqlite::params![
                    commit.cid.to_string(),
                    commit.rev,
                    now,
                    expected.to_string()
                ],
            )?;
            if changed != 1 {
                return Err(ConcurrentWriteError.into());
            }
        }
    }
    put_many_in(conn, &commit.new_blocks, &commit.rev)?;
    delete_many_in(conn, &commit.removed_cids.to_list())?;
    Ok(())
}

fn block_range_in(
    conn: &Connection,
    since: &Option<String>,
    cursor: &Option<CidAndRev>,
) -> Result<Vec<RepoBlock>> {
    let mut sql =
        String::from("SELECT cid, \"repoRev\", size, content FROM repo_block WHERE 1 = 1");
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(cursor) = cursor {
        // use this syntax to ensure we hit the index
        sql.push_str(" AND ((\"repoRev\", cid) < (?, ?))");
        params.push(Box::new(cursor.rev.clone()));
        params.push(Box::new(cursor.cid.to_string()));
    }
    if let Some(since) = since {
        sql.push_str(" AND \"repoRev\" > ?");
        params.push(Box::new(since.clone()));
    }
    sql.push_str(&format!(
        " ORDER BY \"repoRev\" DESC, cid DESC LIMIT {EXPORT_PAGE}"
    ));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(
            rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
            |row| {
                Ok(RepoBlock {
                    cid: row.get(0)?,
                    repo_rev: row.get(1)?,
                    size: row.get(2)?,
                    content: row.get(3)?,
                })
            },
        )?
        .collect::<Result<Vec<RepoBlock>, rusqlite::Error>>()?;
    Ok(rows)
}

#[derive(Clone, Debug)]
pub struct SqlRepoReader {
    pub cache: Arc<RwLock<BlockMap>>,
    pub db: ActorDb,
    pub now: String,
    pub did: String,
}

impl ReadableBlockstore for SqlRepoReader {
    fn get_bytes<'a>(
        &'a self,
        cid: &'a Cid,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Vec<u8>>>> + Send + Sync + 'a>> {
        let db = self.db.clone();
        let cid = *cid;

        Box::pin(async move {
            let cached = {
                let cache_guard = self.cache.read().await;
                cache_guard.get(cid).cloned()
            };
            if let Some(cached_result) = cached {
                return Ok(Some(cached_result));
            }

            let found: Option<Vec<u8>> = db
                .run(move |conn| {
                    Ok(conn
                        .query_row(
                            "SELECT content FROM repo_block WHERE cid = ?1",
                            [cid.to_string()],
                            |row| row.get(0),
                        )
                        .optional()?)
                })
                .await?;
            match found {
                None => Ok(None),
                Some(result) => {
                    {
                        let mut cache_guard = self.cache.write().await;
                        cache_guard.set(cid, result.clone());
                    }
                    Ok(Some(result))
                }
            }
        })
    }

    fn has<'a>(
        &'a self,
        cid: Cid,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + Sync + 'a>> {
        Box::pin(async move {
            let got = <Self as ReadableBlockstore>::get_bytes(self, &cid).await?;
            Ok(got.is_some())
        })
    }

    fn get_blocks<'a>(
        &'a self,
        cids: Vec<Cid>,
    ) -> Pin<Box<dyn Future<Output = Result<BlocksAndMissing>> + Send + Sync + 'a>> {
        let db = self.db.clone();

        Box::pin(async move {
            let cached = {
                let mut cache_guard = self.cache.write().await;
                cache_guard.get_many(cids)?
            };
            if cached.missing.is_empty() {
                return Ok(cached);
            }
            let missing_strings: Vec<String> =
                cached.missing.iter().map(|c| c.to_string()).collect();
            let mut missing = CidSet::new(Some(cached.missing));

            let rows: Vec<(String, Vec<u8>)> = db
                .run(move |conn| {
                    let mut rows = Vec::new();
                    for batch in missing_strings.chunks(500) {
                        let sql = format!(
                            "SELECT cid, content FROM repo_block WHERE cid IN ({})",
                            placeholders(batch.len())
                        );
                        let mut stmt = conn.prepare(&sql)?;
                        let batch_rows = stmt
                            .query_map(rusqlite::params_from_iter(batch.iter()), |row| {
                                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
                            })?
                            .collect::<Result<Vec<(String, Vec<u8>)>, rusqlite::Error>>()?;
                        rows.extend(batch_rows);
                    }
                    Ok(rows)
                })
                .await?;

            let mut blocks = BlockMap::new();
            for (cid_str, content) in rows {
                let cid = Cid::from_str(&cid_str)?;
                blocks.set(cid, content);
                missing.delete(cid);
            }

            {
                let mut cache_guard = self.cache.write().await;
                cache_guard.add_map(blocks.clone())?;
            }
            blocks.add_map(cached.blocks)?;

            Ok(BlocksAndMissing {
                blocks,
                missing: missing.to_list(),
            })
        })
    }
}

impl RepoStorage for SqlRepoReader {
    fn get_root<'a>(&'a self) -> Pin<Box<dyn Future<Output = Option<Cid>> + Send + Sync + 'a>> {
        Box::pin(async move {
            match self.get_root_detailed().await {
                Ok(root) => Some(root.cid),
                Err(_) => None,
            }
        })
    }

    fn put_block<'a>(
        &'a self,
        cid: Cid,
        bytes: Vec<u8>,
        rev: String,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + 'a>> {
        let db = self.db.clone();
        let bytes_cloned = bytes.clone();
        Box::pin(async move {
            db.run(move |conn| {
                conn.execute(
                    "INSERT INTO repo_block (cid, \"repoRev\", size, content) \
                     VALUES (?1, ?2, ?3, ?4) ON CONFLICT DO NOTHING",
                    rusqlite::params![cid.to_string(), rev, bytes.len() as i64, bytes],
                )?;
                Ok(())
            })
            .await?;
            {
                let mut cache_guard = self.cache.write().await;
                cache_guard.set(cid, bytes_cloned);
            }
            Ok(())
        })
    }

    fn put_many<'a>(
        &'a self,
        to_put: BlockMap,
        rev: String,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + 'a>> {
        let db = self.db.clone();

        Box::pin(async move {
            let blocks: Vec<(String, Vec<u8>)> = to_put
                .map
                .iter()
                .map(|(cid, bytes)| (cid.to_string(), bytes.0.clone()))
                .collect();
            db.run(move |conn| {
                let mut stmt = conn.prepare(
                    "INSERT INTO repo_block (cid, \"repoRev\", size, content) \
                     VALUES (?1, ?2, ?3, ?4) ON CONFLICT DO NOTHING",
                )?;
                for (cid, bytes) in &blocks {
                    stmt.execute(rusqlite::params![cid, rev, bytes.len() as i64, bytes])?;
                }
                Ok(())
            })
            .await
        })
    }

    fn update_root<'a>(
        &'a self,
        cid: Cid,
        rev: String,
        is_create: Option<bool>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + 'a>> {
        let did = self.did.clone();
        let db = self.db.clone();
        let now = self.now.clone();

        Box::pin(async move {
            let is_create = is_create.unwrap_or(false);
            db.run(move |conn| {
                if is_create {
                    conn.execute(
                        "INSERT INTO repo_root (did, cid, rev, \"indexedAt\") \
                         VALUES (?1, ?2, ?3, ?4)",
                        rusqlite::params![did, cid.to_string(), rev, now],
                    )?;
                } else {
                    conn.execute(
                        "UPDATE repo_root SET cid = ?1, rev = ?2, \"indexedAt\" = ?3",
                        rusqlite::params![cid.to_string(), rev, now],
                    )?;
                }
                Ok(())
            })
            .await
        })
    }

    fn apply_commit<'a>(
        &'a self,
        commit: CommitData,
        is_create: Option<bool>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + 'a>> {
        Box::pin(async move {
            let expected_root = match is_create.unwrap_or(false) {
                true => None,
                false => Some(self.get_root_detailed().await?.cid),
            };
            let did = self.did.clone();
            let now = self.now.clone();
            let new_blocks = commit.new_blocks.clone();
            self.db
                .tx(move |tx| apply_commit_in(tx, &did, &now, &commit, expected_root.as_ref()))
                .await?;
            let mut cache_guard = self.cache.write().await;
            cache_guard.add_map(new_blocks)?;
            Ok(())
        })
    }
}

impl SqlRepoReader {
    pub fn new(did: String, now: Option<String>, db: ActorDb) -> Self {
        let now = now.unwrap_or_else(rsky_common::now);
        SqlRepoReader {
            cache: Arc::new(RwLock::new(BlockMap::new())),
            db,
            now,
            did,
        }
    }

    /// Exports the repository as a CAR from one read snapshot, so blocks
    /// written while the export runs are neither mixed in nor lost. The
    /// snapshot is a dedicated read-only connection, which leaves the
    /// store's write connection free for the duration.
    pub async fn get_car_stream(&self, since: Option<String>) -> Result<Vec<u8>> {
        self.get_car_stream_within(since, EXPORT_DEADLINE).await
    }

    pub async fn get_car_stream_within(
        &self,
        since: Option<String>,
        deadline: Duration,
    ) -> Result<Vec<u8>> {
        let stream = self.car_stream_within(since, deadline).await?;
        futures::pin_mut!(stream);
        let mut car = Vec::new();
        while let Some(chunk) = futures::StreamExt::next(&mut stream).await {
            car.extend(chunk?);
        }
        Ok(car)
    }

    /// The export as a stream of CAR bytes, read from one snapshot on a
    /// dedicated read-only connection in the reference's block order. The
    /// snapshot is released when the stream is dropped, so an abandoned
    /// download stops the walk.
    pub async fn car_stream_within(
        &self,
        since: Option<String>,
        deadline: Duration,
    ) -> Result<impl Stream<Item = Result<Vec<u8>>> + Send + 'static> {
        let Some(path) = self.db.path().filter(|path| !path.as_os_str().is_empty()) else {
            anyhow::bail!("repository export needs a file-backed store")
        };
        let (root_tx, root_rx) = tokio::sync::oneshot::channel::<Result<Cid>>();
        let (block_tx, mut block_rx) = tokio::sync::mpsc::channel::<Result<Vec<(Cid, Vec<u8>)>>>(4);
        tokio::task::spawn_blocking(move || {
            let mut root_tx = Some(root_tx);
            let mut walk = || -> Result<()> {
                let conn = Connection::open_with_flags(
                    &path,
                    OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
                )?;
                conn.busy_timeout(Duration::from_millis(5000))?;
                let snapshot = conn.unchecked_transaction()?;
                let root: Option<String> = snapshot
                    .query_row("SELECT cid FROM repo_root LIMIT 1", [], |row| row.get(0))
                    .optional()?;
                let Some(root) = root else {
                    return Err(anyhow::Error::new(RepoRootNotFoundError));
                };
                let root = Cid::from_str(&root)?;
                let _ = root_tx.take().expect("root sent once").send(Ok(root));
                let started = Instant::now();
                let mut cursor: Option<CidAndRev> = None;
                loop {
                    if started.elapsed() >= deadline {
                        anyhow::bail!("repository export exceeded {deadline:?}");
                    }
                    let rows = block_range_in(&snapshot, &since, &cursor)?;
                    let Some(last_row) = rows.last() else {
                        break;
                    };
                    cursor = Some(CidAndRev {
                        cid: Cid::from_str(&last_row.cid)?,
                        rev: last_row.repo_rev.clone(),
                    });
                    let batch = rows
                        .iter()
                        .map(|row| Ok((Cid::from_str(&row.cid)?, row.content.clone())))
                        .collect::<Result<Vec<_>>>()?;
                    if block_tx.blocking_send(Ok(batch)).is_err() {
                        // the download was abandoned
                        break;
                    }
                }
                snapshot.rollback()?;
                Ok(())
            };
            if let Err(err) = walk() {
                match root_tx.take() {
                    Some(root_tx) => {
                        let _ = root_tx.send(Err(err));
                    }
                    None => {
                        let _ = block_tx.blocking_send(Err(err));
                    }
                }
            }
        });
        let root = root_rx.await??;
        Ok(rsky_repo::car::write_car_stream(
            Some(&root),
            move |mut writer| async move {
                while let Some(batch) = block_rx.recv().await {
                    for (cid, bytes) in batch? {
                        writer.write(cid, bytes).await?;
                    }
                }
                Ok(writer)
            },
        ))
    }

    pub async fn get_block_range(
        &self,
        since: &Option<String>,
        cursor: &Option<CidAndRev>,
    ) -> Result<Vec<RepoBlock>> {
        let since = since.clone();
        let cursor = cursor.clone();
        self.db
            .run(move |conn| block_range_in(conn, &since, &cursor))
            .await
    }

    pub async fn count_blocks(&self) -> Result<i64> {
        self.db
            .run(
                |conn| Ok(conn.query_row("SELECT count(*) FROM repo_block", [], |row| row.get(0))?),
            )
            .await
    }

    // Transactors
    // -------------------

    /// Proactively cache all blocks from a particular commit (to prevent multiple roundtrips)
    pub async fn cache_rev(&mut self, rev: String) -> Result<()> {
        let db = self.db.clone();
        let res: Vec<(String, Vec<u8>)> = db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT cid, content FROM repo_block WHERE \"repoRev\" = ?1 LIMIT 15",
                )?;
                let rows = stmt
                    .query_map([rev.clone()], |row| Ok((row.get(0)?, row.get(1)?)))?
                    .collect::<Result<Vec<(String, Vec<u8>)>, rusqlite::Error>>()?;
                Ok(rows)
            })
            .await?;
        let mut cache_guard = self.cache.write().await;
        for row in res {
            cache_guard.set(Cid::from_str(&row.0)?, row.1)
        }
        Ok(())
    }

    pub async fn delete_many(&self, cids: Vec<Cid>) -> Result<()> {
        if cids.is_empty() {
            return Ok(());
        }
        let cid_strings: Vec<String> = cids.into_iter().map(|c| c.to_string()).collect();
        self.db
            .run(move |conn| {
                for batch in cid_strings.chunks(500) {
                    let sql = format!(
                        "DELETE FROM repo_block WHERE cid IN ({})",
                        placeholders(batch.len())
                    );
                    conn.execute(&sql, rusqlite::params_from_iter(batch.iter()))?;
                }
                Ok(())
            })
            .await
    }

    pub async fn get_root_detailed(&self) -> Result<CidAndRev> {
        let res: (String, String) = self
            .db
            .run(|conn| {
                Ok(
                    conn.query_row("SELECT cid, rev FROM repo_root LIMIT 1", [], |row| {
                        Ok((row.get(0)?, row.get(1)?))
                    })?,
                )
            })
            .await?;
        Ok(CidAndRev {
            cid: Cid::from_str(&res.0)?,
            rev: res.1,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor_store::db::get_migrated_db;

    fn cid_for(value: &[u8]) -> Cid {
        use sha2::{Digest, Sha256};
        rsky_common::ipld::sha256_to_cid(Sha256::digest(value).to_vec())
    }

    async fn test_reader() -> (tempfile::TempDir, SqlRepoReader) {
        let dir = tempfile::tempdir().unwrap();
        let db = get_migrated_db(dir.path().join("store.sqlite"))
            .await
            .unwrap();
        let reader = SqlRepoReader::new("did:example:alice".to_owned(), None, db);
        (dir, reader)
    }

    #[tokio::test]
    async fn put_and_get_blocks_with_cache() {
        let (_dir, reader) = test_reader().await;
        let bytes = b"block-one".to_vec();
        let cid = cid_for(&bytes);
        reader
            .put_block(cid, bytes.clone(), "rev-1".to_owned())
            .await
            .unwrap();
        // put_block is idempotent
        reader
            .put_block(cid, bytes.clone(), "rev-1".to_owned())
            .await
            .unwrap();
        assert_eq!(reader.get_bytes(&cid).await.unwrap(), Some(bytes.clone()));
        // cached path
        assert_eq!(reader.get_bytes(&cid).await.unwrap(), Some(bytes.clone()));
        assert!(reader.has(cid).await.unwrap());
        let missing_cid = cid_for(b"missing");
        assert!(!reader.has(missing_cid).await.unwrap());
        assert_eq!(reader.count_blocks().await.unwrap(), 1);

        let got = reader.get_blocks(vec![cid, missing_cid]).await.unwrap();
        assert_eq!(got.blocks.get(cid), Some(&bytes));
        assert_eq!(got.missing, vec![missing_cid]);
    }

    #[tokio::test]
    async fn fresh_reader_fetches_from_db_and_caches() {
        let (_dir, reader) = test_reader().await;
        let bytes = b"persisted".to_vec();
        let cid = cid_for(&bytes);
        reader
            .put_block(cid, bytes.clone(), "rev-1".to_owned())
            .await
            .unwrap();

        // a second reader over the same db starts with a cold cache
        let fresh = SqlRepoReader::new(reader.did.clone(), None, reader.db.clone());
        assert_eq!(fresh.get_bytes(&cid).await.unwrap(), Some(bytes.clone()));

        // a third reader exercises the multi-block db path, then the all-cached path
        let fresh_two = SqlRepoReader::new(reader.did.clone(), None, reader.db.clone());
        let first = fresh_two.get_blocks(vec![cid]).await.unwrap();
        assert!(first.missing.is_empty());
        let second = fresh_two.get_blocks(vec![cid]).await.unwrap();
        assert!(second.missing.is_empty());
        assert_eq!(second.blocks.get(cid), Some(&bytes));
    }

    #[tokio::test]
    async fn put_many_and_delete_many() {
        let (_dir, reader) = test_reader().await;
        let mut blocks = BlockMap::new();
        let one = b"one".to_vec();
        let two = b"two".to_vec();
        let (cid_one, cid_two) = (cid_for(&one), cid_for(&two));
        blocks.set(cid_one, one.clone());
        blocks.set(cid_two, two.clone());
        reader.put_many(blocks, "rev-1".to_owned()).await.unwrap();
        assert_eq!(reader.count_blocks().await.unwrap(), 2);

        let fetched = reader.get_blocks(vec![cid_one, cid_two]).await.unwrap();
        assert!(fetched.missing.is_empty());
        assert_eq!(fetched.blocks.get(cid_one), Some(&one));

        reader.delete_many(vec![]).await.unwrap();
        reader.delete_many(vec![cid_one]).await.unwrap();
        assert_eq!(reader.count_blocks().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn root_lifecycle() {
        let (_dir, reader) = test_reader().await;
        assert!(reader.get_root().await.is_none());
        assert!(reader.get_root_detailed().await.is_err());

        let root_one = cid_for(b"root-one");
        reader
            .update_root(root_one, "rev-1".to_owned(), Some(true))
            .await
            .unwrap();
        assert_eq!(reader.get_root().await, Some(root_one));

        let root_two = cid_for(b"root-two");
        reader
            .update_root(root_two, "rev-2".to_owned(), None)
            .await
            .unwrap();
        let detailed = reader.get_root_detailed().await.unwrap();
        assert_eq!(detailed.cid, root_two);
        assert_eq!(detailed.rev, "rev-2");
    }

    #[tokio::test]
    async fn apply_commit_writes_and_removes() {
        let (_dir, reader) = test_reader().await;
        let removed = b"removed".to_vec();
        let removed_cid = cid_for(&removed);
        reader
            .put_block(removed_cid, removed, "rev-1".to_owned())
            .await
            .unwrap();
        reader
            .update_root(removed_cid, "rev-1".to_owned(), Some(true))
            .await
            .unwrap();

        let added = b"added".to_vec();
        let added_cid = cid_for(&added);
        let mut new_blocks = BlockMap::new();
        new_blocks.set(added_cid, added.clone());
        let commit = CommitData {
            cid: added_cid,
            rev: "rev-2".to_owned(),
            since: Some("rev-1".to_owned()),
            prev: Some(removed_cid),
            new_blocks,
            relevant_blocks: BlockMap::new(),
            removed_cids: CidSet::new(Some(vec![removed_cid])),
        };
        reader.apply_commit(commit, None).await.unwrap();

        let detailed = reader.get_root_detailed().await.unwrap();
        assert_eq!(detailed.rev, "rev-2");
        assert_eq!(reader.count_blocks().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn cache_rev_populates_cache() {
        let (_dir, mut reader) = test_reader().await;
        let bytes = b"cache-me".to_vec();
        let cid = cid_for(&bytes);
        reader
            .put_block(cid, bytes.clone(), "rev-9".to_owned())
            .await
            .unwrap();
        reader.cache_rev("rev-9".to_owned()).await.unwrap();
        let cache_guard = reader.cache.read().await;
        assert_eq!(cache_guard.get(cid), Some(&bytes));
    }

    #[tokio::test]
    async fn apply_commit_creates_the_root() {
        let (_dir, reader) = test_reader().await;
        let bytes = b"genesis".to_vec();
        let cid = cid_for(&bytes);
        let mut new_blocks = BlockMap::new();
        new_blocks.set(cid, bytes);
        let commit = CommitData {
            cid,
            rev: "rev-1".to_owned(),
            since: None,
            prev: None,
            new_blocks,
            relevant_blocks: BlockMap::new(),
            removed_cids: CidSet::new(None),
        };
        reader.apply_commit(commit, Some(true)).await.unwrap();
        assert_eq!(reader.get_root().await, Some(cid));
        assert_eq!(reader.count_blocks().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn apply_commit_rejects_a_moved_root() {
        let (_dir, reader) = test_reader().await;
        let root_one = cid_for(b"root-one");
        reader
            .update_root(root_one, "rev-1".to_owned(), Some(true))
            .await
            .unwrap();
        let stale = cid_for(b"stale");
        let commit = CommitData {
            cid: cid_for(b"root-two"),
            rev: "rev-2".to_owned(),
            since: Some("rev-1".to_owned()),
            prev: Some(stale),
            new_blocks: BlockMap::new(),
            relevant_blocks: BlockMap::new(),
            removed_cids: CidSet::new(None),
        };
        let did = reader.did.clone();
        let err = reader
            .db
            .tx(move |tx| apply_commit_in(tx, &did, "now", &commit, Some(&stale)))
            .await
            .unwrap_err();
        assert_eq!(
            err.downcast_ref::<ConcurrentWriteError>(),
            Some(&ConcurrentWriteError)
        );
        assert_eq!(reader.get_root_detailed().await.unwrap().rev, "rev-1");
    }

    #[tokio::test]
    async fn car_stream_is_a_snapshot_with_a_deadline() {
        let (_dir, reader) = test_reader().await;
        let root_bytes = b"snapshot-root".to_vec();
        let root_cid = cid_for(&root_bytes);
        reader
            .put_block(root_cid, root_bytes, "rev-1".to_owned())
            .await
            .unwrap();
        reader
            .update_root(root_cid, "rev-1".to_owned(), Some(true))
            .await
            .unwrap();
        let car = reader.get_car_stream(None).await.unwrap();
        assert!(!car.is_empty());
        let expired = reader
            .get_car_stream_within(None, Duration::ZERO)
            .await
            .unwrap_err();
        assert!(expired.to_string().contains("exceeded"));

        let memory = SqlRepoReader::new(
            reader.did.clone(),
            None,
            crate::db::sqlite::Db::open(":memory:").unwrap(),
        );
        let err = memory.get_car_stream(None).await.unwrap_err();
        assert!(err.to_string().contains("file-backed"));
    }

    #[tokio::test]
    async fn an_abandoned_export_stops_the_walk() {
        let (_dir, reader) = test_reader().await;
        let root_bytes = b"big-root".to_vec();
        let root_cid = cid_for(&root_bytes);
        let mut blocks = BlockMap::new();
        blocks.set(root_cid, root_bytes);
        for i in 0..3000u32 {
            let bytes = format!("block-{i}").into_bytes();
            blocks.set(cid_for(&bytes), bytes);
        }
        reader.put_many(blocks, "rev-1".to_owned()).await.unwrap();
        reader
            .update_root(root_cid, "rev-1".to_owned(), Some(true))
            .await
            .unwrap();
        let stream = reader
            .car_stream_within(None, EXPORT_DEADLINE)
            .await
            .unwrap();
        // never read; the producer's pending batch send fails once the
        // writer gives up on the dropped body
        drop(stream);
        tokio::time::sleep(Duration::from_millis(200)).await;
        let full = reader.get_car_stream(None).await.unwrap();
        assert!(full.len() > 3000 * 10);
    }

    #[tokio::test]
    async fn car_stream_and_block_range() {
        let (_dir, reader) = test_reader().await;
        assert!(reader.get_car_stream(None).await.is_err());

        let root_bytes = b"the-root".to_vec();
        let root_cid = cid_for(&root_bytes);
        reader
            .put_block(root_cid, root_bytes, "rev-1".to_owned())
            .await
            .unwrap();
        let second = b"second".to_vec();
        let second_cid = cid_for(&second);
        reader
            .put_block(second_cid, second, "rev-2".to_owned())
            .await
            .unwrap();
        reader
            .update_root(root_cid, "rev-2".to_owned(), Some(true))
            .await
            .unwrap();

        let car = reader.get_car_stream(None).await.unwrap();
        assert!(!car.is_empty());
        let car_since = reader.get_car_stream(Some("rev-1".to_owned())).await;
        assert!(car_since.is_ok());

        let range = reader.get_block_range(&None, &None).await.unwrap();
        assert_eq!(range.len(), 2);
        assert_eq!(range[0].repo_rev, "rev-2");
        let cursor = Some(CidAndRev {
            cid: Cid::from_str(&range[0].cid).unwrap(),
            rev: range[0].repo_rev.clone(),
        });
        let rest = reader.get_block_range(&None, &cursor).await.unwrap();
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].repo_rev, "rev-1");
        let since = reader
            .get_block_range(&Some("rev-1".to_owned()), &None)
            .await
            .unwrap();
        assert_eq!(since.len(), 1);
        assert_eq!(since[0].repo_rev, "rev-2");
    }
}
