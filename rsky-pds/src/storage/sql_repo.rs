use std::str::FromStr;

use anyhow::Result;
use diesel::dsl::sql;
use diesel::prelude::*;
use diesel::sql_types::{Bool, Text};
use diesel::*;
use lexicon_cid::Cid;

use crate::car::read_car_bytes;
use crate::db::establish_connection;
use crate::models::RepoBlock;
use crate::repo::block_map::{BlockMap, BlocksAndMissing};
use crate::repo::cid_set::CidSet;
use crate::repo::types::CommitData;
use crate::storage::readable_blockstore::ReadableBlockstore;
use crate::storage::types::RepoStorage;
use crate::storage::CidAndRev;
use crate::storage::RepoRootError::RepoRootNotFoundError;
use crate::{common, models};

#[derive(Clone, Debug)]
pub struct SqlRepoReader {
    pub cache: BlockMap,
    pub root: Option<Cid>,
    pub rev: Option<String>,
    pub now: String,
    pub did: String, // @TODO: Refactor so SQL Repo Reader reads from one repo
}

impl ReadableBlockstore for SqlRepoReader {
    fn get_bytes(&mut self, cid: &Cid) -> Result<Option<Vec<u8>>> {
        use crate::schema::pds::repo_block::dsl as RepoBlockSchema;
        let conn = &mut establish_connection()?;

        let cached = self.cache.get(*cid);
        if let Some(cached_result) = cached {
            return Ok(Some(cached_result.clone()));
        }

        let found: Option<Vec<u8>> = RepoBlockSchema::repo_block
            .filter(RepoBlockSchema::cid.eq(cid.to_string()))
            .filter(RepoBlockSchema::did.eq(&self.did))
            .select(RepoBlockSchema::content)
            .first(conn)
            .optional()?;
        match found {
            None => Ok(None),
            Some(result) => {
                self.cache.set(*cid, result.clone());
                Ok(Some(result))
            }
        }
    }

    fn has(&mut self, cid: Cid) -> Result<bool> {
        let got = <Self as ReadableBlockstore>::get_bytes(self, &cid)?;
        Ok(got.is_some())
    }

    fn get_blocks(&mut self, cids: Vec<Cid>) -> Result<BlocksAndMissing> {
        use crate::schema::pds::repo_block::dsl as RepoBlockSchema;
        let conn = &mut establish_connection()?;

        let cached = self.cache.get_many(cids)?;

        if cached.missing.len() < 1 {
            return Ok(cached);
        }
        let mut missing = CidSet::new(Some(cached.missing.clone()));
        let missing_strings: Vec<String> =
            cached.missing.into_iter().map(|c| c.to_string()).collect();

        let mut blocks = BlockMap::new();

        let _: Vec<_> = missing_strings
            .chunks(500)
            .into_iter()
            .map(|batch| {
                let _: Vec<_> = RepoBlockSchema::repo_block
                    .filter(RepoBlockSchema::cid.eq_any(batch))
                    .filter(RepoBlockSchema::did.eq(&self.did))
                    .select((RepoBlockSchema::cid, RepoBlockSchema::content))
                    .load::<(String, Vec<u8>)>(conn)?
                    .into_iter()
                    .map(|row: (String, Vec<u8>)| {
                        let cid = Cid::from_str(&row.0).unwrap();
                        blocks.set(cid, row.1);
                        missing.delete(cid);
                    })
                    .collect();
                Ok::<(), anyhow::Error>(())
            })
            .collect();
        self.cache.add_map(blocks.clone())?;
        blocks.add_map(cached.blocks)?;
        Ok(BlocksAndMissing {
            blocks,
            missing: missing.to_list(),
        })
    }
}

impl RepoStorage for SqlRepoReader {
    fn get_root(&self) -> Option<Cid> {
        match self.get_root_detailed() {
            Ok(root) => Some(root.cid),
            Err(_) => None,
        }
    }

    fn put_block(&mut self, cid: Cid, bytes: Vec<u8>, rev: String) -> Result<()> {
        use crate::schema::pds::repo_block::dsl as RepoBlockSchema;
        let conn = &mut establish_connection()?;

        insert_into(RepoBlockSchema::repo_block)
            .values((
                RepoBlockSchema::did.eq(&self.did),
                RepoBlockSchema::cid.eq(cid.to_string()),
                RepoBlockSchema::repoRev.eq(rev),
                RepoBlockSchema::size.eq(bytes.len() as i32),
                RepoBlockSchema::content.eq(bytes.clone()),
            ))
            .execute(conn)?;
        self.cache.set(cid, bytes);
        Ok(())
    }

    fn put_many(&mut self, to_put: BlockMap, rev: String) -> Result<()> {
        use crate::schema::pds::repo_block::dsl as RepoBlockSchema;
        let conn = &mut establish_connection()?;

        let mut blocks: Vec<RepoBlock> = Vec::new();
        for (cid, bytes) in to_put.map.iter() {
            blocks.push(RepoBlock {
                cid: cid.to_string(),
                did: self.did.clone(),
                repo_rev: rev.clone(),
                size: bytes.0.len() as i32,
                content: bytes.0.clone(),
            });
        }
        let _ = blocks
            .chunks(50)
            .map(|batch| {
                Ok(insert_into(RepoBlockSchema::repo_block)
                    .values(batch)
                    .on_conflict_do_nothing()
                    .execute(conn)?)
            })
            .collect::<Result<Vec<usize>>>()?;
        Ok(())
    }

    fn update_root(&mut self, cid: Cid, rev: String, is_create: Option<bool>) -> Result<()> {
        use crate::schema::pds::repo_root::dsl as RepoRootSchema;
        let conn = &mut establish_connection()?;

        let is_create = is_create.unwrap_or(false);
        if is_create {
            insert_into(RepoRootSchema::repo_root)
                .values((
                    RepoRootSchema::did.eq(&self.did),
                    RepoRootSchema::cid.eq(cid.to_string()),
                    RepoRootSchema::rev.eq(rev),
                    RepoRootSchema::indexedAt.eq(&self.now),
                ))
                .execute(conn)?;
        } else {
            update(RepoRootSchema::repo_root)
                .filter(RepoRootSchema::did.eq(&self.did))
                .set((
                    RepoRootSchema::cid.eq(cid.to_string()),
                    RepoRootSchema::rev.eq(rev),
                    RepoRootSchema::indexedAt.eq(&self.now),
                ))
                .execute(conn)?;
        }
        Ok(())
    }

    fn apply_commit(&mut self, commit: CommitData, is_create: Option<bool>) -> Result<()> {
        self.update_root(commit.cid, commit.rev.clone(), is_create)?;
        self.put_many(commit.new_blocks, commit.rev)?;
        self.delete_many(commit.removed_cids.to_list())?;
        Ok(())
    }
}

// Basically handles getting ipld blocks from db
impl SqlRepoReader {
    pub fn new(did: String, now: Option<String>) -> Self {
        let now = now.unwrap_or_else(|| common::now());
        SqlRepoReader {
            cache: BlockMap::new(),
            root: None,
            rev: None,
            now,
            did,
        }
    }

    pub async fn get_car_stream(&self, since: Option<String>) -> Result<Vec<u8>> {
        match self.get_root() {
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
                read_car_bytes(Some(&root), car).await
            }
        }
    }

    pub async fn get_block_range(
        &self,
        since: &Option<String>,
        cursor: &Option<CidAndRev>,
    ) -> Result<Vec<RepoBlock>> {
        use crate::schema::pds::repo_block::dsl as RepoBlockSchema;
        let conn = &mut establish_connection()?;

        let mut builder = RepoBlockSchema::repo_block
            .select(RepoBlock::as_select())
            .order((RepoBlockSchema::repoRev.desc(), RepoBlockSchema::cid.desc()))
            .filter(RepoBlockSchema::did.eq(&self.did))
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
        Ok(builder.load(conn)?)
    }

    pub async fn count_blocks(&self) -> Result<i64> {
        use crate::schema::pds::repo_block::dsl as RepoBlockSchema;
        let conn = &mut establish_connection()?;

        let res = RepoBlockSchema::repo_block
            .filter(RepoBlockSchema::did.eq(&self.did))
            .count()
            .get_result(conn)?;
        Ok(res)
    }

    // Transactors
    // -------------------

    /// Proactively cache all blocks from a particular commit (to prevent multiple roundtrips)
    pub async fn cache_rev(&mut self, rev: String) -> Result<()> {
        use crate::schema::pds::repo_block::dsl as RepoBlockSchema;
        let conn = &mut establish_connection()?;

        let res: Vec<(String, Vec<u8>)> = RepoBlockSchema::repo_block
            .filter(RepoBlockSchema::did.eq(&self.did))
            .filter(RepoBlockSchema::repoRev.eq(rev))
            .select((RepoBlockSchema::cid, RepoBlockSchema::content))
            .limit(15)
            .get_results::<(String, Vec<u8>)>(conn)?;
        for row in res {
            self.cache.set(Cid::from_str(&row.0)?, row.1)
        }
        Ok(())
    }

    pub fn delete_many(&self, cids: Vec<Cid>) -> Result<()> {
        if cids.len() < 1 {
            return Ok(());
        }
        use crate::schema::pds::repo_block::dsl as RepoBlockSchema;
        let conn = &mut establish_connection()?;

        let cid_strings: Vec<String> = cids.into_iter().map(|c| c.to_string()).collect();
        delete(RepoBlockSchema::repo_block)
            .filter(RepoBlockSchema::cid.eq_any(cid_strings))
            .execute(conn)?;
        Ok(())
    }

    pub fn get_root_detailed(&self) -> Result<CidAndRev> {
        use crate::schema::pds::repo_root::dsl as RepoRootSchema;
        let conn = &mut establish_connection()?;

        let res = RepoRootSchema::repo_root
            .filter(RepoRootSchema::did.eq(&self.did))
            .select(models::RepoRoot::as_select())
            .first(conn)?;

        Ok(CidAndRev {
            cid: Cid::from_str(&res.cid)?,
            rev: res.rev,
        })
    }
}
