use crate::common;
use anyhow::Result;
use lexicon_cid::Cid;
use libipld::cbor::DagCborCodec;
use libipld::codec::{Codec, Encode};
use libipld::multihash::{Code, MultihashDigest};
use libipld::raw::RawCodec;
use libipld::{Block, DefaultParams};
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

pub fn sha256_to_cid<T: Codec>(hash: Vec<u8>, codec: T) -> Cid
where
    u64: From<T>,
{
    let cid = Cid::new_v1(u64::from(codec), Code::Sha2_256.digest(hash.as_slice()));
    cid
}

pub fn sha256_raw_to_cid(hash: Vec<u8>) -> Cid {
    sha256_to_cid(hash, RawCodec)
}
