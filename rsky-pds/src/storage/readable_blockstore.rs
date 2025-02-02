use crate::repo::block_map::BlocksAndMissing;
use crate::repo::error::DataStoreError;
use crate::repo::parse;
use crate::repo::types::RepoRecord;
use crate::repo::util::cbor_to_lex_record;
use crate::storage::ObjAndBytes;
use anyhow::Result;
use lexicon_cid::Cid;
use serde_cbor::Value as CborValue;

pub trait ReadableBlockstore: Send + Sync {
    fn get_bytes(&mut self, cid: &Cid) -> Result<Option<Vec<u8>>>;
    fn has(&mut self, cid: Cid) -> Result<bool>; // mutable to include cacheing
    fn get_blocks(&mut self, cids: Vec<Cid>) -> Result<BlocksAndMissing>;

    fn attempt_read(
        &mut self,
        cid: &Cid,
        check: Box<dyn Fn(&CborValue) -> bool>,
    ) -> Result<Option<ObjAndBytes>> {
        let bytes = match self.get_bytes(cid) {
            Ok(Some(bytes)) => bytes,
            _ => return Ok(None),
        };
        parse::parse_obj_by_kind(bytes, *cid, move |v| check(v))
            .map(Some)
            .map_err(Into::into)
    }

    fn read_obj_and_bytes(
        &mut self,
        cid: &Cid,
        check: Box<dyn Fn(&CborValue) -> bool>,
    ) -> Result<ObjAndBytes> {
        self.attempt_read(cid, check)?
            .ok_or_else(|| DataStoreError::MissingBlock(cid.to_string()).into())
    }

    fn read_obj(&mut self, cid: &Cid, check: Box<dyn Fn(&CborValue) -> bool>) -> Result<CborValue> {
        Ok(self.read_obj_and_bytes(cid, check)?.obj)
    }

    fn attempt_read_record(&mut self, cid: &Cid) -> Option<RepoRecord> {
        match self.read_record(cid) {
            Ok(res) => Some(res),
            Err(_) => None,
        }
    }

    fn read_record(&mut self, cid: &Cid) -> Result<RepoRecord> {
        let bytes = self.get_bytes(cid)?;
        bytes
            .map(|bytes| cbor_to_lex_record(bytes))
            .transpose()?
            .ok_or_else(|| DataStoreError::MissingBlock(cid.to_string()).into())
    }
}
