use crate::repo::block_map::{BlockMap, BlocksAndMissing};
use crate::repo::types::CommitData;
use crate::storage::readable_blockstore::ReadableBlockstore;
use crate::storage::types::RepoStorage;
use anyhow::Result;
use lexicon_cid::Cid;
use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone, Debug)]
pub struct MemoryBlockstore {
    pub blocks: Arc<RwLock<BlockMap>>,
    pub root: Arc<RwLock<Option<Cid>>>,
    pub rev: Arc<RwLock<Option<String>>>,
}

impl Default for MemoryBlockstore {
    fn default() -> Self {
        Self {
            blocks: Arc::new(RwLock::new(BlockMap::new())),
            root: Arc::new(RwLock::new(None)),
            rev: Arc::new(RwLock::new(None)),
        }
    }
}

impl MemoryBlockstore {
    pub async fn new(blocks: Option<BlockMap>) -> Result<Self> {
        let this = Self::default();
        if let Some(blocks) = blocks {
            let mut block_guard = this.blocks.write().await;
            block_guard.add_map(blocks)?;
        }
        Ok(this)
    }
}

impl ReadableBlockstore for MemoryBlockstore {
    fn get_bytes<'a>(
        &'a self,
        cid: &'a Cid,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Vec<u8>>>> + Send + Sync + 'a>> {
        Box::pin(async move {
            let block_guard = self.blocks.read().await;
            match block_guard.get(*cid) {
                None => Ok(None),
                Some(res) => Ok(Some(res.clone())),
            }
        })
    }

    fn has<'a>(
        &'a self,
        cid: Cid,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + Sync + 'a>> {
        Box::pin(async move {
            let block_guard = self.blocks.read().await;
            Ok(block_guard.has(cid))
        })
    }

    fn get_blocks<'a>(
        &'a self,
        cids: Vec<Cid>,
    ) -> Pin<Box<dyn Future<Output = Result<BlocksAndMissing>> + Send + Sync + 'a>> {
        Box::pin(async move {
            let mut block_guard = self.blocks.write().await;
            block_guard.get_many(cids)
        })
    }
}

impl RepoStorage for MemoryBlockstore {
    fn get_root<'a>(&'a self) -> Pin<Box<dyn Future<Output = Option<Cid>> + Send + Sync + 'a>> {
        Box::pin(async move {
            let root_guard = self.root.read().await;
            *root_guard
        })
    }

    fn put_block<'a>(
        &'a self,
        cid: Cid,
        bytes: Vec<u8>,
        _rev: String,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + 'a>> {
        Box::pin(async move {
            let mut block_guard = self.blocks.write().await;
            Ok(block_guard.set(cid, bytes))
        })
    }

    fn put_many<'a>(
        &'a self,
        to_put: BlockMap,
        _rev: String,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + 'a>> {
        Box::pin(async move {
            let mut block_guard = self.blocks.write().await;
            block_guard.add_map(to_put)
        })
    }

    fn update_root<'a>(
        &'a self,
        cid: Cid,
        rev: String,
        _is_create: Option<bool>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + 'a>> {
        Box::pin(async move {
            let mut root_guard = self.root.write().await;
            *root_guard = Some(cid);
            let mut rev_guard = self.rev.write().await;
            *rev_guard = Some(rev);
            Ok(())
        })
    }

    fn apply_commit<'a>(
        &'a self,
        commit: CommitData,
        _is_create: Option<bool>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + 'a>> {
        Box::pin(async move {
            let mut root_guard = self.root.write().await;
            *root_guard = Some(commit.cid);
            let rm_cids = commit.removed_cids.to_list();

            let mut block_guard = self.blocks.write().await;
            for cid in rm_cids {
                block_guard.delete(cid)?;
            }
            for (cid, bytes) in commit.new_blocks.map.iter() {
                block_guard.set(Cid::from_str(cid)?, bytes.0.clone());
            }
            Ok(())
        })
    }
}
