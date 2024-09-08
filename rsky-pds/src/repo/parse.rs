use crate::repo::block_map::BlockMap;
use crate::repo::error::DataStoreError;
use crate::repo::types::RepoRecord;
use crate::repo::util::cbor_to_lex_record;
use crate::storage::ObjAndBytes;
use anyhow::Result;
use lexicon_cid::Cid;
use serde_cbor::Value as CborValue;

pub struct RecordAndBytes {
    pub record: RepoRecord,
    pub bytes: Vec<u8>,
}

pub fn get_and_parse_record(blocks: &BlockMap, cid: Cid) -> Result<RecordAndBytes> {
    let bytes = blocks.get(cid);
    return if let Some(b) = bytes {
        let record = cbor_to_lex_record(b.clone())?;
        Ok(RecordAndBytes {
            record,
            bytes: b.clone(),
        })
    } else {
        Err(anyhow::Error::new(DataStoreError::MissingBlock(
            cid.to_string(),
        )))
    };
}

pub fn get_and_parse_by_kind(
    blocks: &BlockMap,
    cid: Cid,
    check: impl Fn(&'_ CborValue) -> bool,
) -> Result<ObjAndBytes> {
    let bytes = blocks.get(cid);
    return if let Some(b) = bytes {
        Ok(parse_obj_by_kind(b.clone(), cid, check)?)
    } else {
        Err(anyhow::Error::new(DataStoreError::MissingBlock(
            cid.to_string(),
        )))
    };
}

pub fn parse_obj_by_kind(
    bytes: Vec<u8>,
    cid: Cid,
    check: impl Fn(&'_ CborValue) -> bool,
) -> Result<ObjAndBytes> {
    let obj: CborValue = serde_ipld_dagcbor::from_slice(bytes.as_slice()).map_err(|error| {
        anyhow::Error::new(DataStoreError::UnexpectedObject(cid)).context(error)
    })?;
    if check(&obj) {
        Ok(ObjAndBytes { obj, bytes })
    } else {
        Err(anyhow::Error::new(DataStoreError::UnexpectedObject(cid)))
    }
}
