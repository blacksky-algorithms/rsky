use crate::block_map::{BlockMap, BlocksAndMissing};
use crate::storage::readable_blockstore::ReadableBlockstore;
use crate::storage::types::RepoStorage;
use crate::types::CommitData;
use anyhow::Result;
use lexicon_cid::Cid;
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug)]
pub struct SyncStorage {
    pub staged: Arc<RwLock<dyn RepoStorage>>,
    pub saved: Arc<RwLock<dyn RepoStorage>>,
}

impl SyncStorage {
    pub fn new(staged: Arc<RwLock<dyn RepoStorage>>, saved: Arc<RwLock<dyn RepoStorage>>) -> Self {
        Self { staged, saved }
    }
}

impl ReadableBlockstore for SyncStorage {
    fn get_bytes<'a>(
        &'a self,
        cid: &'a Cid,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Vec<u8>>>> + Send + Sync + 'a>> {
        Box::pin(async move {
            let staged_guard = self.staged.read().await;
            match staged_guard.get_bytes(cid).await {
                Ok(Some(got)) => Ok(Some(got)),
                _ => {
                    let saved_guard = self.saved.read().await;
                    saved_guard.get_bytes(cid).await
                }
            }
        })
    }

    fn has<'a>(
        &'a self,
        cid: Cid,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + Sync + 'a>> {
        Box::pin(async move {
            let staged_guard = self.staged.read().await;
            let saved_guard = self.saved.read().await;
            Ok(staged_guard.has(cid).await? || saved_guard.has(cid).await?)
        })
    }

    fn get_blocks<'a>(
        &'a self,
        cids: Vec<Cid>,
    ) -> Pin<Box<dyn Future<Output = Result<BlocksAndMissing>> + Send + Sync + 'a>> {
        Box::pin(async move {
            let staged_guard = self.staged.read().await;
            let saved_guard = self.saved.read().await;
            let from_staged = staged_guard.get_blocks(cids).await?;
            let from_saved = saved_guard.get_blocks(from_staged.missing).await?;
            let mut blocks = from_staged.blocks;
            blocks.add_map(from_saved.blocks)?;
            Ok(BlocksAndMissing {
                blocks,
                missing: from_saved.missing,
            })
        })
    }
}

// Ideally this would only implement ReadableBlockstore but at the time
// trait upcasting was not yet available
impl RepoStorage for SyncStorage {
    fn get_root<'a>(&'a self) -> Pin<Box<dyn Future<Output = Option<Cid>> + Send + Sync + 'a>> {
        unimplemented!()
    }

    fn put_block<'a>(
        &'a self,
        _cid: Cid,
        _bytes: Vec<u8>,
        _rev: String,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + 'a>> {
        unimplemented!()
    }

    fn put_many<'a>(
        &'a self,
        _to_put: BlockMap,
        _rev: String,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + 'a>> {
        unimplemented!()
    }

    fn update_root<'a>(
        &'a self,
        _cid: Cid,
        _rev: String,
        _is_create: Option<bool>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + 'a>> {
        unimplemented!()
    }

    fn apply_commit<'a>(
        &'a self,
        _commit: CommitData,
        _is_create: Option<bool>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + 'a>> {
        unimplemented!()
    }
}
