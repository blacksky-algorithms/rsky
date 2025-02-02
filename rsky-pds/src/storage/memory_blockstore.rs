use crate::repo::block_map::{BlockMap, BlocksAndMissing};
use crate::repo::types::CommitData;
use crate::storage::readable_blockstore::ReadableBlockstore;
use crate::storage::types::RepoStorage;
use anyhow::Result;
use lexicon_cid::Cid;
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
}
