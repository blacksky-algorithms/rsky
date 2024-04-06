use crate::common;
use anyhow::Result;
use libipld::cbor::DagCborCodec;
use libipld::codec::Encode;
use libipld::multihash::{Code, MultihashDigest};
use libipld::{Block, Cid, DefaultParams};
use serde::Serialize;

pub fn cid_for_cbor<T: Serialize>(data: &T) -> Result<Cid> {
    let bytes = common::struct_to_cbor(data)?;
    let cid = Cid::new_v1(
        u64::from(DagCborCodec),
        Code::Sha2_256.digest(bytes.as_slice()),
    );
    Ok(cid)
}

pub fn data_to_cbor_block<T: Encode<DagCborCodec>>(data: &T) -> Result<Block<DefaultParams>> {
    Block::<DefaultParams>::encode(DagCborCodec, Code::Blake3_256, data)
}
