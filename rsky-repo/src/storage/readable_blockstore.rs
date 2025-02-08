use crate::block_map::BlocksAndMissing;
use crate::error::DataStoreError;
use crate::parse;
use crate::storage::ObjAndBytes;
use crate::types::RepoRecord;
use crate::util::cbor_to_lex_record;
use anyhow::Result;
use lexicon_cid::Cid;
use serde_cbor::Value as CborValue;
use std::future::Future;
use std::pin::Pin;

pub trait ReadableBlockstore: Send + Sync {
    fn get_bytes<'a>(
        &'a self,
        cid: &'a Cid,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Vec<u8>>>> + Send + Sync + 'a>>;
    fn has<'a>(
        &'a self,
        cid: Cid,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + Sync + 'a>>;
    fn get_blocks<'a>(
        &'a self,
        cids: Vec<Cid>,
    ) -> Pin<Box<dyn Future<Output = Result<BlocksAndMissing>> + Send + Sync + 'a>>;

    fn attempt_read<'a>(
        &'a self,
        cid: &'a Cid,
        check: Box<dyn Fn(CborValue) -> bool + Send + Sync>,
    ) -> Pin<Box<dyn Future<Output = Result<Option<ObjAndBytes>>> + Send + Sync + 'a>> {
        Box::pin(async move {
            let bytes = match self.get_bytes(cid).await {
                Ok(Some(bytes)) => bytes,
                _ => return Ok(None),
            };
            parse::parse_obj_by_kind(bytes, *cid, move |v| check(v))
                .map(Some)
                .map_err(Into::into)
        })
    }

    fn read_obj_and_bytes<'a>(
        &'a self,
        cid: &'a Cid,
        check: Box<dyn Fn(CborValue) -> bool + Send + Sync>,
    ) -> Pin<Box<dyn Future<Output = Result<ObjAndBytes>> + Send + Sync + 'a>> {
        Box::pin(async move {
            self.attempt_read(cid, check)
                .await?
                .ok_or_else(|| DataStoreError::MissingBlock(cid.to_string()).into())
        })
    }

    fn read_obj<'a>(
        &'a self,
        cid: &'a Cid,
        check: Box<dyn Fn(CborValue) -> bool + Send + Sync>,
    ) -> Pin<Box<dyn Future<Output = Result<CborValue>> + Send + Sync + 'a>> {
        Box::pin(async move { Ok(self.read_obj_and_bytes(cid, check).await?.obj) })
    }

    fn attempt_read_record<'a>(
        &'a self,
        cid: &'a Cid,
    ) -> Pin<Box<dyn Future<Output = Option<RepoRecord>> + Send + Sync + 'a>> {
        Box::pin(async move {
            match self.read_record(cid).await {
                Ok(res) => Some(res),
                Err(_) => None,
            }
        })
    }

    fn read_record<'a>(
        &'a self,
        cid: &'a Cid,
    ) -> Pin<Box<dyn Future<Output = Result<RepoRecord>> + Send + Sync + 'a>> {
        Box::pin(async move {
            let bytes = self.get_bytes(cid).await?;
            bytes
                .map(|bytes| cbor_to_lex_record(bytes))
                .transpose()?
                .ok_or_else(|| DataStoreError::MissingBlock(cid.to_string()).into())
        })
    }
}
