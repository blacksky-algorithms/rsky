use crate::repo::block_map::{BlockMap, BlocksAndMissing};
use crate::repo::types::{CommitData, RepoRecord};
use crate::storage::ObjAndBytes;
use anyhow::Result;
use lexicon_cid::Cid;
use serde_cbor::Value as CborValue;

pub trait RepoStorage {
    // Writeable
    fn get_root(&self) -> Option<Cid>;
    fn put_block(&mut self, cid: Cid, bytes: Vec<u8>, rev: String) -> Result<()>;
    fn put_many(&mut self, to_put: BlockMap, rev: String) -> Result<()>;
    fn update_root(&mut self, cid: Cid, rev: String, is_create: Option<bool>) -> Result<()>;
    fn apply_commit(&mut self, commit: CommitData, is_create: Option<bool>) -> Result<()>;

    // Readable
    fn get_bytes(&mut self, cid: &Cid) -> Result<Option<Vec<u8>>>;
    fn has(&mut self, cid: Cid) -> Result<bool>;
    fn get_blocks(&mut self, cids: Vec<Cid>) -> Result<BlocksAndMissing>;
    fn attempt_read(
        &mut self,
        cid: &Cid,
        check: impl Fn(&'_ CborValue) -> bool + Send + Sync,
    ) -> Result<Option<ObjAndBytes>>;
    fn read_obj_and_bytes(
        &mut self,
        cid: &Cid,
        check: impl Fn(&'_ CborValue) -> bool + Send + Sync,
    ) -> Result<ObjAndBytes>;
    fn read_obj(
        &mut self,
        cid: &Cid,
        check: impl Fn(&'_ CborValue) -> bool + Send + Sync,
    ) -> Result<CborValue>;
    fn attempt_read_record(&mut self, cid: &Cid) -> Option<RepoRecord>;
    fn read_record(&mut self, cid: &Cid) -> Result<RepoRecord>;
}
