use crate::repo::block_map::BlockMap;
use crate::repo::types::CommitData;
use crate::storage::readable_blockstore::ReadableBlockstore;
use anyhow::Result;
use lexicon_cid::Cid;
use std::fmt::Debug;

pub trait RepoStorage: ReadableBlockstore + Send + Sync + Debug {
    // Writeable
    fn get_root(&self) -> Option<Cid>;
    fn put_block(&mut self, cid: Cid, bytes: Vec<u8>, rev: String) -> Result<()>;
    fn put_many(&mut self, to_put: BlockMap, rev: String) -> Result<()>;
    fn update_root(&mut self, cid: Cid, rev: String, is_create: Option<bool>) -> Result<()>;
    fn apply_commit(&mut self, commit: CommitData, is_create: Option<bool>) -> Result<()>;
}
