// based on atproto/packages/pds/src/actor-store/repo/sql-repo-reader.ts

use crate::db::establish_connection;
use crate::models::RepoBlock;
use crate::repo::block_map::{BlockMap, BlocksAndMissing};
use crate::repo::cid_set::CidSet;
use crate::repo::error::DataStoreError;
use crate::repo::mst::NodeData;
use crate::repo::parse;
use crate::repo::types::{CommitData, RepoRecord, VersionedCommit};
use crate::repo::util::cbor_to_lex_record;
use crate::{common, models};
use anyhow::Result;
use diesel::prelude::*;
use diesel::*;
use futures::try_join;
use libipld::Cid;
use std::collections::BTreeMap;
use std::str::FromStr;

/// Ipld
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum Ipld {
    /// Represents the absence of a value or the value undefined.
    Null,
    /// Represents a boolean value.
    Bool(bool),
    /// Represents an integer.
    Integer(i128),
    /// Represents a floating point value.
    Float(f64),
    /// Represents a UTF-8 string.
    String(String),
    /// Represents a sequence of bytes.
    Bytes(Vec<u8>),
    /// Represents a list.
    List(Vec<Ipld>),
    /// Represents a map of strings.
    Map(BTreeMap<String, Ipld>),
    /// Represents a map of integers.
    Link(Cid),
    /// Represents MST Node.
    Node(NodeData),
    /// Represents a commit,
    VersionedCommit(VersionedCommit),
}

impl Ipld {
    pub fn node(self) -> NodeData {
        if let Ipld::Node(s) = self {
            s
        } else {
            panic!("Not a NodeData")
        }
    }

    pub fn commit(self) -> VersionedCommit {
        if let Ipld::VersionedCommit(s) = self {
            s
        } else {
            panic!("Not a VersionedCommit")
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ObjAndBytes {
    pub obj: Ipld,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CidAndRev {
    pub cid: Cid,
    pub rev: String,
}

#[derive(Clone)]
pub struct SqlRepoReader {
    pub cache: BlockMap,
    pub blocks: BlockMap,
    pub root: Option<Cid>,
    pub rev: Option<String>,
    pub now: String,
    pub did: String, // @TODO: Refactor so SQL Repo Reader reads from one repo
}

// Basically handles getting ipld blocks from db
impl SqlRepoReader {
    pub fn new(blocks: Option<BlockMap>, did: String, now: Option<String>) -> Self {
        let now = now.unwrap_or_else(|| common::now());
        let mut this = SqlRepoReader {
            cache: BlockMap::new(),
            blocks: BlockMap::new(),
            root: None,
            rev: None,
            now,
            did,
        };
        if let Some(blocks) = blocks {
            this.blocks.add_map(blocks).unwrap();
        }
        this
    }

    pub async fn get_blocks(&mut self, cids: Vec<Cid>) -> Result<BlocksAndMissing> {
        use crate::schema::pds::repo_block::dsl as RepoBlockSchema;
        let conn = &mut establish_connection()?;

        let cached = self.cache.get_many(cids)?;
        if cached.missing.len() < 1 {
            return Ok(cached);
        }
        let missing = CidSet::new(Some(cached.missing.clone()));
        let missing_strings: Vec<String> =
            cached.missing.into_iter().map(|c| c.to_string()).collect();
        let mut blocks = BlockMap::new();

        let _ = missing_strings.chunks(500).map(|batch| {
            let _ = RepoBlockSchema::repo_block
                .filter(RepoBlockSchema::cid.eq_any(batch))
                .filter(RepoBlockSchema::did.eq(&self.did))
                .select((RepoBlockSchema::cid, RepoBlockSchema::content))
                .load::<(String, Vec<u8>)>(conn)?
                .into_iter()
                .map(|row: (String, Vec<u8>)| {
                    let cid = Cid::from_str(&row.0).unwrap();
                    blocks.set(cid, row.1);
                });
            Ok::<(), anyhow::Error>(())
        });
        self.cache.add_map(blocks.clone())?;
        blocks.add_map(cached.blocks)?;
        Ok(BlocksAndMissing {
            blocks,
            missing: missing.to_list(),
        })
    }

    pub fn get_bytes(&mut self, cid: &Cid) -> Result<Vec<u8>> {
        use crate::schema::pds::repo_block::dsl as RepoBlockSchema;
        let conn = &mut establish_connection()?;

        let cached = self.cache.get(*cid);
        if let Some(cached_result) = cached {
            return Ok(cached_result.clone());
        }

        let result: Vec<u8> = RepoBlockSchema::repo_block
            .filter(RepoBlockSchema::cid.eq(cid.to_string()))
            .filter(RepoBlockSchema::did.eq(&self.did))
            .select(RepoBlockSchema::content)
            .first(conn)
            .map_err(|_| anyhow::Error::new(DataStoreError::MissingBlock(cid.to_string())))?;
        self.cache.set(*cid, result.clone());
        Ok(result)
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

    pub fn has(&mut self, cid: Cid) -> Result<bool> {
        let got = self.get_bytes(&cid)?;
        Ok(!got.is_empty())
    }

    pub fn attempt_read(
        &mut self,
        cid: &Cid,
        check: impl Fn(&'_ Ipld) -> bool,
    ) -> Result<ObjAndBytes> {
        let bytes = self.get_bytes(cid)?;
        Ok(parse::parse_obj_by_kind(bytes, *cid, check)?)
    }

    pub fn read_obj_and_bytes(
        &mut self,
        cid: &Cid,
        check: impl Fn(&'_ Ipld) -> bool,
    ) -> Result<ObjAndBytes> {
        let read = self.attempt_read(cid, check)?;
        Ok(read)
    }

    pub fn read_obj(&mut self, cid: &Cid, check: impl Fn(&'_ Ipld) -> bool) -> Result<Ipld> {
        let obj = self.read_obj_and_bytes(cid, check)?;
        Ok(obj.obj)
    }

    pub fn read_record(&mut self, cid: &Cid) -> Result<RepoRecord> {
        let bytes = self.get_bytes(cid)?;
        Ok(cbor_to_lex_record(bytes)?)
    }

    // Transactors
    // -------------------

    pub async fn apply_commit(
        &mut self,
        commit: CommitData,
        is_create: Option<bool>,
    ) -> Result<()> {
        try_join!(
            self.update_root(commit.cid, commit.rev.clone(), is_create),
            self.put_many(commit.new_blocks, commit.rev),
            self.delete_many(commit.removed_cids.to_list())
        )?;
        Ok(())
    }

    pub async fn put_many(&self, to_put: BlockMap, rev: String) -> Result<()> {
        use crate::schema::pds::repo_block::dsl as RepoBlockSchema;
        let conn = &mut establish_connection()?;

        let mut blocks: Vec<RepoBlock> = Vec::new();
        for (cid, bytes) in to_put.map.iter() {
            blocks.push(RepoBlock {
                cid: cid.to_string(),
                did: self.did.clone(),
                repo_rev: rev.clone(),
                size: bytes.len() as i32,
                content: bytes.clone(),
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

    pub async fn delete_many(&self, cids: Vec<Cid>) -> Result<()> {
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

    pub async fn update_root(&self, cid: Cid, rev: String, is_create: Option<bool>) -> Result<()> {
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
                .set((
                    RepoRootSchema::cid.eq(cid.to_string()),
                    RepoRootSchema::rev.eq(rev),
                    RepoRootSchema::indexedAt.eq(&self.now),
                ))
                .execute(conn)?;
        }
        Ok(())
    }

    pub async fn get_root(&self) -> Option<Cid> {
        match self.get_root_detailed().await {
            Ok(root) => Some(root.cid),
            Err(_) => None,
        }
    }

    pub async fn get_root_detailed(&self) -> Result<CidAndRev> {
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
