use crate::repo::block_map::BlockMap;
use crate::repo::types::CommitData;
use crate::storage::readable_blockstore::ReadableBlockstore;
use anyhow::Result;
use lexicon_cid::Cid;
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;

pub trait RepoStorage: ReadableBlockstore + Send + Sync + Debug {
    // Writeable
    fn get_root<'a>(&'a self) -> Pin<Box<dyn Future<Output = Option<Cid>> + Send + Sync + 'a>>;
    fn put_block<'a>(
        &'a self,
        cid: Cid,
        bytes: Vec<u8>,
        rev: String,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + 'a>>;
    fn put_many<'a>(
        &'a self,
        to_put: BlockMap,
        rev: String,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + 'a>>;
    fn update_root<'a>(
        &'a self,
        cid: Cid,
        rev: String,
        is_create: Option<bool>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + 'a>>;
    fn apply_commit<'a>(
        &'a self,
        commit: CommitData,
        is_create: Option<bool>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + 'a>>;
}
