use anyhow::Result;
use lexicon_cid::Cid;
use libipld::codec::Codec;
use libipld::raw::RawCodec;
use multihash::Multihash;
use serde::Serialize;
use sha2::{Digest, Sha256};

const SHA2_256: u64 = 0x12;
const DAGCBORCODEC: u64 = 0x71;

pub fn cid_for_cbor<T: Serialize>(data: &T) -> Result<Cid> {
    let bytes = crate::struct_to_cbor(data)?;
    let mut sha = Sha256::new();
    sha.update(&bytes);
    let hash = sha.finalize();
    let cid = Cid::new_v1(
        DAGCBORCODEC,
        Multihash::<64>::wrap(SHA2_256, hash.as_slice())?,
    );
    Ok(cid)
}

pub fn sha256_to_cid<T: Codec>(hash: Vec<u8>, codec: T) -> Cid
where
    u64: From<T>,
{
    let cid = Cid::new_v1(
        u64::from(codec),
        Multihash::<64>::wrap(SHA2_256, hash.as_slice()).unwrap(),
    );
    cid
}

pub fn sha256_raw_to_cid(hash: Vec<u8>) -> Cid {
    sha256_to_cid(hash, RawCodec)
}
