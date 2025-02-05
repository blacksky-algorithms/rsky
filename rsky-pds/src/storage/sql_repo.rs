use crate::car::blocks_to_car_file;
use crate::db::DbConn;
use crate::models::RepoBlock;
use crate::repo::block_map::{BlockMap, BlocksAndMissing};
use crate::repo::cid_set::CidSet;
use crate::repo::types::CommitData;
use crate::storage::readable_blockstore::ReadableBlockstore;
use crate::storage::types::RepoStorage;
use crate::storage::CidAndRev;
use crate::storage::RepoRootError::RepoRootNotFoundError;
use crate::{common, models};
use anyhow::Result;
use diesel::dsl::sql;
use diesel::prelude::*;
use diesel::sql_types::{Bool, Text};
use diesel::*;
use futures::{stream, StreamExt, TryStreamExt};
use lexicon_cid::Cid;
use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone, Debug)]
pub struct SqlRepoReader {
    pub cache: Arc<RwLock<BlockMap>>,
    pub db: Arc<DbConn>,
    pub root: Option<Cid>,
    pub rev: Option<String>,
    pub now: String,
    pub did: String,
}

impl ReadableBlockstore for SqlRepoReader {
    fn get_bytes<'a>(
        &'a self,
        cid: &'a Cid,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Vec<u8>>>> + Send + Sync + 'a>> {
        let did: String = self.did.clone();
        let db: Arc<DbConn> = self.db.clone();
        let cid = cid.clone();

        Box::pin(async move {
            use crate::schema::pds::repo_block::dsl as RepoBlockSchema;
            let cached = {
                let cache_guard = self.cache.read().await;
                match cache_guard.get(cid) {
                    None => None,
                    Some(v) => Some(v.clone()),
                }
            };
            if let Some(cached_result) = cached {
                return Ok(Some(cached_result.clone()));
            }

            let found: Option<Vec<u8>> = db
                .run(move |conn| {
                    RepoBlockSchema::repo_block
                        .filter(RepoBlockSchema::cid.eq(cid.to_string()))
                        .filter(RepoBlockSchema::did.eq(did))
                        .select(RepoBlockSchema::content)
                        .first(conn)
                        .optional()
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
        let did: String = self.did.clone();
        let db: Arc<DbConn> = self.db.clone();

        Box::pin(async move {
            use crate::schema::pds::repo_block::dsl as RepoBlockSchema;
            let cached = {
                let mut cache_guard = self.cache.write().await;
                cache_guard.get_many(cids)?
            };

            if cached.missing.len() < 1 {
                return Ok(cached);
            }
            let missing = CidSet::new(Some(cached.missing.clone()));
            let missing_strings: Vec<String> =
                cached.missing.into_iter().map(|c| c.to_string()).collect();

            let blocks = Arc::new(tokio::sync::Mutex::new(BlockMap::new()));
            let missing_set = Arc::new(tokio::sync::Mutex::new(missing));

            let _: Vec<_> = stream::iter(missing_strings.chunks(500))
                .then(|batch| {
                    let this_db = db.clone();
                    let this_did = did.clone();
                    let blocks = Arc::clone(&blocks);
                    let missing = Arc::clone(&missing_set);
                    let batch = batch.to_vec(); // Convert to owned Vec

                    async move {
                        // Database query
                        let rows: Vec<(String, Vec<u8>)> = this_db
                            .run(move |conn| {
                                RepoBlockSchema::repo_block
                                    .filter(RepoBlockSchema::cid.eq_any(batch))
                                    .filter(RepoBlockSchema::did.eq(this_did))
                                    .select((RepoBlockSchema::cid, RepoBlockSchema::content))
                                    .load(conn)
                            })
                            .await?;

                        // Process rows with locked access
                        let mut blocks = blocks.lock().await;
                        let mut missing = missing.lock().await;

                        for row in rows {
                            let cid = Cid::from_str(&row.0)?; // Proper error handling
                            blocks.set(cid, row.1);
                            missing.delete(cid);
                        }

                        Ok::<(), anyhow::Error>(())
                    }
                })
                .try_collect()
                .await?;

            // Extract values from synchronization primitives
            let mut blocks = Arc::try_unwrap(blocks)
                .expect("Arc still has owners")
                .into_inner();
            let missing = Arc::try_unwrap(missing_set)
                .expect("Arc still has owners")
                .into_inner();

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
        let did: String = self.did.clone();
        let db: Arc<DbConn> = self.db.clone();
        let bytes_cloned = bytes.clone();
        Box::pin(async move {
            use crate::schema::pds::repo_block::dsl as RepoBlockSchema;

            db.run(move |conn| {
                insert_into(RepoBlockSchema::repo_block)
                    .values((
                        RepoBlockSchema::did.eq(did),
                        RepoBlockSchema::cid.eq(cid.to_string()),
                        RepoBlockSchema::repoRev.eq(rev),
                        RepoBlockSchema::size.eq(bytes.len() as i32),
                        RepoBlockSchema::content.eq(bytes),
                    ))
                    .execute(conn)
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
        let did: String = self.did.clone();
        let db: Arc<DbConn> = self.db.clone();

        Box::pin(async move {
            use crate::schema::pds::repo_block::dsl as RepoBlockSchema;

            let blocks: Vec<RepoBlock> = to_put
                .map
                .iter()
                .map(|(cid, bytes)| RepoBlock {
                    cid: cid.to_string(),
                    did: did.clone(),
                    repo_rev: rev.clone(),
                    size: bytes.0.len() as i32,
                    content: bytes.0.clone(),
                })
                .collect();

            let chunks: Vec<Vec<RepoBlock>> =
                blocks.chunks(50).map(|chunk| chunk.to_vec()).collect();

            let _: Vec<_> = stream::iter(chunks)
                .then(|batch| {
                    let db = db.clone();
                    async move {
                        db.run(move |conn| {
                            insert_into(RepoBlockSchema::repo_block)
                                .values(batch)
                                .on_conflict_do_nothing()
                                .execute(conn)
                                .map(|_| ())
                        })
                        .await
                        .map_err(anyhow::Error::from)
                    }
                })
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .collect::<Result<Vec<()>>>()?;

            Ok(())
        })
    }
    fn update_root<'a>(
        &'a self,
        cid: Cid,
        rev: String,
        is_create: Option<bool>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + 'a>> {
        let did: String = self.did.clone();
        let db: Arc<DbConn> = self.db.clone();
        let now: String = self.now.clone();

        Box::pin(async move {
            use crate::schema::pds::repo_root::dsl as RepoRootSchema;

            let is_create = is_create.unwrap_or(false);
            if is_create {
                db.run(move |conn| {
                    insert_into(RepoRootSchema::repo_root)
                        .values((
                            RepoRootSchema::did.eq(did),
                            RepoRootSchema::cid.eq(cid.to_string()),
                            RepoRootSchema::rev.eq(rev),
                            RepoRootSchema::indexedAt.eq(now),
                        ))
                        .execute(conn)
                })
                .await?;
            } else {
                db.run(move |conn| {
                    update(RepoRootSchema::repo_root)
                        .filter(RepoRootSchema::did.eq(did))
                        .set((
                            RepoRootSchema::cid.eq(cid.to_string()),
                            RepoRootSchema::rev.eq(rev),
                            RepoRootSchema::indexedAt.eq(now),
                        ))
                        .execute(conn)
                })
                .await?;
            }
            Ok(())
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

// Basically handles getting ipld blocks from db
impl SqlRepoReader {
    pub fn new(did: String, now: Option<String>, db: DbConn) -> Self {
        let now = now.unwrap_or_else(|| common::now());
        SqlRepoReader {
            cache: Arc::new(RwLock::new(BlockMap::new())),
            root: None,
            rev: None,
            db: Arc::new(db),
            now,
            did,
        }
    }

    pub async fn get_car_stream(&self, since: Option<String>) -> Result<Vec<u8>> {
        match self.get_root().await {
            None => return Err(anyhow::Error::new(RepoRootNotFoundError)),
            Some(root) => {
                let mut car = BlockMap::new();
                let mut cursor: Option<CidAndRev> = None;
                let mut write_rows = |rows: Vec<RepoBlock>| -> Result<()> {
                    for row in rows {
                        car.set(Cid::from_str(&row.cid)?, row.content);
                    }
                    Ok(())
                };
                loop {
                    let res = self.get_block_range(&since, &cursor).await?;
                    write_rows(res.clone())?;
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
        let did: String = self.did.clone();
        let db: Arc<DbConn> = self.db.clone();
        let since = since.clone();
        let cursor = cursor.clone();
        use crate::schema::pds::repo_block::dsl as RepoBlockSchema;

        Ok(db
            .run(move |conn| {
                let mut builder = RepoBlockSchema::repo_block
                    .select(RepoBlock::as_select())
                    .order((RepoBlockSchema::repoRev.desc(), RepoBlockSchema::cid.desc()))
                    .filter(RepoBlockSchema::did.eq(did))
                    .limit(500)
                    .into_boxed();

                if let Some(cursor) = cursor {
                    // use this syntax to ensure we hit the index
                    builder = builder.filter(
                        sql::<Bool>("((")
                            .bind(RepoBlockSchema::repoRev)
                            .sql(", ")
                            .bind(RepoBlockSchema::cid)
                            .sql(") < (")
                            .bind::<Text, _>(cursor.rev.clone())
                            .sql(", ")
                            .bind::<Text, _>(cursor.cid.to_string())
                            .sql("))"),
                    );
                }
                if let Some(since) = since {
                    builder = builder.filter(RepoBlockSchema::repoRev.gt(since));
                }
                builder.load(conn)
            })
            .await?)
    }

    pub async fn count_blocks(&self) -> Result<i64> {
        let did: String = self.did.clone();
        let db: Arc<DbConn> = self.db.clone();
        use crate::schema::pds::repo_block::dsl as RepoBlockSchema;

        let res = db
            .run(move |conn| {
                RepoBlockSchema::repo_block
                    .filter(RepoBlockSchema::did.eq(did))
                    .count()
                    .get_result(conn)
            })
            .await?;
        Ok(res)
    }

    // Transactors
    // -------------------

    /// Proactively cache all blocks from a particular commit (to prevent multiple roundtrips)
    pub async fn cache_rev(&mut self, rev: String) -> Result<()> {
        let did: String = self.did.clone();
        let db: Arc<DbConn> = self.db.clone();
        use crate::schema::pds::repo_block::dsl as RepoBlockSchema;

        let res: Vec<(String, Vec<u8>)> = db
            .run(move |conn| {
                RepoBlockSchema::repo_block
                    .filter(RepoBlockSchema::did.eq(did))
                    .filter(RepoBlockSchema::repoRev.eq(rev))
                    .select((RepoBlockSchema::cid, RepoBlockSchema::content))
                    .limit(15)
                    .get_results::<(String, Vec<u8>)>(conn)
            })
            .await?;
        for row in res {
            let mut cache_guard = self.cache.write().await;
            cache_guard.set(Cid::from_str(&row.0)?, row.1)
        }
        Ok(())
    }

    pub async fn delete_many(&self, cids: Vec<Cid>) -> Result<()> {
        if cids.len() < 1 {
            return Ok(());
        }
        let did: String = self.did.clone();
        let db: Arc<DbConn> = self.db.clone();
        use crate::schema::pds::repo_block::dsl as RepoBlockSchema;

        let cid_strings: Vec<String> = cids.into_iter().map(|c| c.to_string()).collect();
        db.run(move |conn| {
            delete(RepoBlockSchema::repo_block)
                .filter(RepoBlockSchema::did.eq(did))
                .filter(RepoBlockSchema::cid.eq_any(cid_strings))
                .execute(conn)
        })
        .await?;
        Ok(())
    }

    pub async fn get_root_detailed(&self) -> Result<CidAndRev> {
        let did: String = self.did.clone();
        let db: Arc<DbConn> = self.db.clone();
        use crate::schema::pds::repo_root::dsl as RepoRootSchema;

        let res = db
            .run(move |conn| {
                RepoRootSchema::repo_root
                    .filter(RepoRootSchema::did.eq(did))
                    .select(models::RepoRoot::as_select())
                    .first(conn)
            })
            .await?;

        Ok(CidAndRev {
            cid: Cid::from_str(&res.cid)?,
            rev: res.rev,
        })
    }
}
