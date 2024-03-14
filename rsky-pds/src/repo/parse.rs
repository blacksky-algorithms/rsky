use crate::repo::block_map::BlockMap;
use crate::repo::error::DataStoreError;
use crate::storage::{Ipld, ObjAndBytes};
use anyhow::Result;
use libipld::Cid;

pub fn get_and_parse_by_kind(
    blocks: &BlockMap,
    cid: Cid,
    check: impl Fn(&'_ Ipld) -> bool,
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
    check: impl Fn(&'_ Ipld) -> bool,
) -> Result<ObjAndBytes> {
    let obj: Ipld = serde_ipld_dagcbor::from_slice(bytes.as_slice()).map_err(|error| {
        anyhow::Error::new(DataStoreError::UnexpectedObject(cid)).context(error)
    })?;
    if check(&obj) {
        Ok(ObjAndBytes { obj, bytes })
    } else {
        Err(anyhow::Error::new(DataStoreError::UnexpectedObject(cid)))
    }
}
