use crate::repo::block_map::{BlockMap, BlocksAndMissing};
use crate::repo::types::{CommitData, RepoRecord};
use crate::storage::readable_blockstore::ReadableBlockstore;
use crate::storage::types::RepoStorage;
use crate::storage::ObjAndBytes;
use anyhow::Result;
use lexicon_cid::Cid;
use serde_cbor::Value as CborValue;
use std::str::FromStr;

#[derive(Clone, Debug)]
pub struct MemoryBlockstore {
    pub blocks: BlockMap,
    pub root: Option<Cid>,
    pub rev: Option<String>,
}

impl Default for MemoryBlockstore {
    fn default() -> Self {
        Self {
            blocks: BlockMap::new(),
            root: None,
            rev: None,
        }
    }
}

impl MemoryBlockstore {
    pub fn new(blocks: Option<BlockMap>) -> Result<Self> {
        let mut this = Self::default();
        if let Some(blocks) = blocks {
            this.blocks.add_map(blocks)?;
        }
        Ok(this)
    }
}

impl ReadableBlockstore for MemoryBlockstore {
    fn get_bytes(&mut self, cid: &Cid) -> Result<Option<Vec<u8>>> {
        match self.blocks.get(*cid) {
            None => Ok(None),
            Some(res) => Ok(Some(res.clone())),
        }
    }

    fn has(&mut self, cid: Cid) -> Result<bool> {
        Ok(self.blocks.has(cid))
    }

    fn get_blocks(&mut self, cids: Vec<Cid>) -> Result<BlocksAndMissing> {
        self.blocks.get_many(cids)
    }
}

impl RepoStorage for MemoryBlockstore {
    fn get_root(&self) -> Option<Cid> {
        self.root
    }

    fn put_block(&mut self, cid: Cid, bytes: Vec<u8>, _rev: String) -> Result<()> {
        Ok(self.blocks.set(cid, bytes))
    }

    fn put_many(&mut self, to_put: BlockMap, _rev: String) -> Result<()> {
        self.blocks.add_map(to_put)
    }

    fn update_root(&mut self, cid: Cid, rev: String, _is_create: Option<bool>) -> Result<()> {
        self.root = Some(cid);
        self.rev = Some(rev);
        Ok(())
    }

    fn apply_commit(&mut self, commit: CommitData, _is_create: Option<bool>) -> Result<()> {
        self.root = Some(commit.cid);
        let rm_cids = commit.removed_cids.to_list();
        for cid in rm_cids {
            self.blocks.delete(cid)?;
        }
        for (cid, bytes) in commit.new_blocks.map.iter() {
            self.blocks.set(Cid::from_str(cid)?, bytes.clone());
        }
        Ok(())
    }

    fn get_bytes(&mut self, cid: &Cid) -> Result<Option<Vec<u8>>> {
        <Self as ReadableBlockstore>::get_bytes(self, cid)
    }

    fn has(&mut self, cid: Cid) -> Result<bool> {
        <Self as ReadableBlockstore>::has(self, cid)
    }

    fn get_blocks(&mut self, cids: Vec<Cid>) -> Result<BlocksAndMissing> {
        <Self as ReadableBlockstore>::get_blocks(self, cids)
    }

    fn attempt_read(
        &mut self,
        cid: &Cid,
        check: impl Fn(&'_ CborValue) -> bool + Send + Sync,
    ) -> Result<Option<ObjAndBytes>> {
        <Self as ReadableBlockstore>::attempt_read(self, cid, check)
    }

    fn read_obj_and_bytes(
        &mut self,
        cid: &Cid,
        check: impl Fn(&'_ CborValue) -> bool + Send + Sync,
    ) -> Result<ObjAndBytes> {
        <Self as ReadableBlockstore>::read_obj_and_bytes(self, cid, check)
    }

    fn read_obj(
        &mut self,
        cid: &Cid,
        check: impl Fn(&'_ CborValue) -> bool + Send + Sync,
    ) -> Result<CborValue> {
        <Self as ReadableBlockstore>::read_obj(self, cid, check)
    }

    fn attempt_read_record(&mut self, cid: &Cid) -> Option<RepoRecord> {
        <Self as ReadableBlockstore>::attempt_read_record(self, cid)
    }

    fn read_record(&mut self, cid: &Cid) -> Result<RepoRecord> {
        <Self as ReadableBlockstore>::read_record(self, cid)
    }
}
