// based on atproto/packages/pds/src/actor-store/repo/sql-repo-reader.ts

use crate::repo::block_map::{BlockMap, BlocksAndMissing};
use crate::repo::error::DataStoreError;
use crate::repo::mst::NodeData;
use crate::repo::parse;
use crate::repo::types::{CommitData, RepoRecord, VersionedCommit};
use crate::repo::util::cbor_to_lex_record;
use anyhow::Result;
use diesel::prelude::*;
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

#[derive(Clone)]
pub struct SqlRepoReader<'a> {
    pub cache: BlockMap,
    pub conn: &'a PgConnection,
    pub blocks: BlockMap,
    pub root: Option<Cid>,
    pub rev: Option<String>,
}

// Basically handles getting ipld blocks from db
impl<'a> SqlRepoReader<'a> {
    pub fn new(conn: &mut PgConnection, blocks: Option<BlockMap>) -> Self {
        let mut this = SqlRepoReader {
            cache: BlockMap::new(),
            conn,
            blocks: BlockMap::new(),
            root: None,
            rev: None,
        };
        if let Some(blocks) = blocks {
            this.blocks.add_map(blocks).unwrap();
        }
        this
    }

    pub fn get_blocks(
        &mut self,
        conn: &mut PgConnection,
        cids: Vec<Cid>,
    ) -> Result<BlocksAndMissing> {
        use crate::schema::pds::repo_block::dsl as RepoBlockSchema;

        let cached = self.cache.get_many(cids);
        if let Ok(cached_result) = cached {
            if cached_result.missing.len() < 1 {
                return Ok(cached_result);
            }
        }
        let mut blocks = BlockMap::new();
        RepoBlockSchema::repo_block
            .select((RepoBlockSchema::cid, RepoBlockSchema::content))
            .load::<(String, Vec<u8>)>(conn)?
            .into_iter()
            .map(|row: (String, Vec<u8>)| {
                let cid = Cid::from_str(&row.0).unwrap();
                blocks.set(cid, row.1);
            })
            .for_each(drop);
        Ok(BlocksAndMissing {
            blocks,
            missing: Vec::new(),
        })
    }

    pub fn get_bytes(&mut self, conn: &mut PgConnection, cid: &Cid) -> Result<Vec<u8>> {
        use crate::schema::pds::repo_block::dsl as RepoBlockSchema;

        let cached = self.cache.get(*cid);
        if let Some(cached_result) = cached {
            return Ok(cached_result.clone());
        }

        let result: Vec<u8> = RepoBlockSchema::repo_block
            .filter(RepoBlockSchema::cid.eq(cid.to_string()))
            .select(RepoBlockSchema::content)
            .first(conn)
            .map_err(|error| anyhow::Error::new(DataStoreError::MissingBlock(cid.to_string())))?;
        self.cache.set(*cid, result.clone());
        Ok(result)
    }

    pub fn has(&mut self, cid: Cid) -> Result<bool> {
        let got = self.get_bytes(&mut self.conn, &cid)?;
        Ok(!got.is_empty())
    }

    pub fn attempt_read(
        &mut self,
        cid: &Cid,
        check: impl Fn(&'_ Ipld) -> bool,
    ) -> Result<ObjAndBytes> {
        let bytes = self.get_bytes(&mut self.conn, cid)?;
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
        let bytes = self.get_bytes(&mut self.conn, cid)?;
        Ok(cbor_to_lex_record(bytes)?)
    }

    // Transactors
    // -------------------

    pub fn apply_commit(&mut self, commit: CommitData) -> Result<()> {
        self.root = Some(commit.cid);
        let rm_cids = commit.removed_cids.to_list();
        for cid in rm_cids {
            self.blocks.delete(cid)?;
        }
        commit.new_blocks.for_each(|bytes, cid| {
            self.blocks.set(cid, bytes.clone());
        })?;
        Ok(())
    }

    pub fn get_root(&self) -> Option<Cid> {
        self.root
    }
}
