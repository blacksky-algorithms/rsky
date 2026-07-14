use crate::actor_store::db::{ActorDb, RepoBlock};
use anyhow::Result;
use lexicon_cid::Cid;
use rsky_common;
use rsky_repo::block_map::{BlockMap, BlocksAndMissing};
use rsky_repo::car::blocks_to_car_file;
use rsky_repo::cid_set::CidSet;
use rsky_repo::storage::readable_blockstore::ReadableBlockstore;
use rsky_repo::storage::types::RepoStorage;
use rsky_repo::storage::CidAndRev;
use rsky_repo::storage::RepoRootError::RepoRootNotFoundError;
use rsky_repo::types::CommitData;
use rusqlite::OptionalExtension;
use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

pub(crate) fn placeholders(len: usize) -> String {
    vec!["?"; len].join(",")
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
            self.update_root(commit.cid, commit.rev.clone(), is_create)
                .await?;
            self.put_many(commit.new_blocks, commit.rev).await?;
            self.delete_many(commit.removed_cids.to_list()).await?;
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

    pub async fn get_car_stream(&self, since: Option<String>) -> Result<Vec<u8>> {
        match self.get_root().await {
            None => Err(anyhow::Error::new(RepoRootNotFoundError)),
            Some(root) => {
                let mut car = BlockMap::new();
                let mut cursor: Option<CidAndRev> = None;
                loop {
                    let res = self.get_block_range(&since, &cursor).await?;
                    for row in &res {
                        car.set(Cid::from_str(&row.cid)?, row.content.clone());
                    }
                    if let Some(last_row) = res.last() {
                        cursor = Some(CidAndRev {
                            cid: Cid::from_str(&last_row.cid)?,
                            rev: last_row.repo_rev.clone(),
                        });
                    } else {
                        break;
                    }
                }
                blocks_to_car_file(Some(&root), car).await
            }
        }
    }

    pub async fn get_block_range(
        &self,
        since: &Option<String>,
        cursor: &Option<CidAndRev>,
    ) -> Result<Vec<RepoBlock>> {
        let db = self.db.clone();
        let since = since.clone();
        let cursor = cursor.clone();

        db.run(move |conn| {
            let mut sql =
                String::from("SELECT cid, \"repoRev\", size, content FROM repo_block WHERE 1 = 1");
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            if let Some(cursor) = &cursor {
                // use this syntax to ensure we hit the index
                sql.push_str(" AND ((\"repoRev\", cid) < (?, ?))");
                params.push(Box::new(cursor.rev.clone()));
                params.push(Box::new(cursor.cid.to_string()));
            }
            if let Some(since) = &since {
                sql.push_str(" AND \"repoRev\" > ?");
                params.push(Box::new(since.clone()));
            }
            sql.push_str(" ORDER BY \"repoRev\" DESC, cid DESC LIMIT 500");
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
        })
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
