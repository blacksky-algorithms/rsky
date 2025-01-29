use crate::repo::block_map::BlocksAndMissing;
use crate::repo::error::DataStoreError;
use crate::repo::parse;
use crate::repo::types::RepoRecord;
use crate::repo::util::cbor_to_lex_record;
use crate::storage::ObjAndBytes;
use anyhow::Result;
use lexicon_cid::Cid;
use serde_cbor::Value as CborValue;

pub trait ReadableBlockstore {
     fn get_bytes(&mut self, cid: &Cid) -> Result<Option<Vec<u8>>>;
     fn has(&mut self, cid: Cid) -> Result<bool>;
     fn get_blocks(&mut self, cids: Vec<Cid>) -> Result<BlocksAndMissing>;

     fn attempt_read(
        &mut self,
        cid: &Cid,
        check: impl Fn(&'_ CborValue) -> bool,
    ) -> Result<Option<ObjAndBytes>> {
        let bytes = match self.get_bytes(cid) {
            Ok(Some(bytes)) => bytes,
            _ => return Ok(None),
        };
        Ok(Some(parse::parse_obj_by_kind(bytes, *cid, check)?))
    }

     fn read_obj_and_bytes(
        &mut self,
        cid: &Cid,
        check: impl Fn(&'_ CborValue) -> bool,
    ) -> Result<ObjAndBytes> {
        let read = self.attempt_read(cid, check)?;
        match read {
            None => Err(anyhow::Error::new(DataStoreError::MissingBlock(
                cid.to_string(),
            ))),
            Some(read) => Ok(read),
        }
    }

     fn read_obj(
        &mut self,
        cid: &Cid,
        check: impl Fn(&'_ CborValue) -> bool,
    ) -> Result<CborValue> {
        let obj = self.read_obj_and_bytes(cid, check)?;
        Ok(obj.obj)
    }

     fn attempt_read_record(&mut self, cid: &Cid) -> Option<RepoRecord> {
        match self.read_record(cid) {
            Ok(res) => Some(res),
            Err(_) => None,
        }
    }

     fn read_record(&mut self, cid: &Cid) -> Result<RepoRecord> {
        let bytes = self.get_bytes(cid)?;
        match bytes {
            None => Err(anyhow::Error::new(DataStoreError::MissingBlock(
                cid.to_string(),
            ))),
            Some(bytes) => Ok(cbor_to_lex_record(bytes)?),
        }
    }
}
