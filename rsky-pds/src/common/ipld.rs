use crate::common;
use anyhow::Result;
use libipld::multihash::{Code, MultihashDigest};
use libipld::Cid;
use serde::Serialize;

const DAG: u64 = 0x71;

pub fn cid_for_cbor<T: Serialize>(data: &T) -> Result<Cid> {
    let bytes = common::struct_to_cbor(data)?;
    let cid = Cid::new_v1(DAG, Code::Sha2_256.digest(bytes.as_slice()));
    Ok(cid)
}
