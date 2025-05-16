use anyhow::Result;
use ipld_core::codec::Codec;
use lexicon_cid::Cid;
use multihash::Multihash;
use serde::Serialize;
use sha2::{Digest, Sha256};

const SHA2_256: u64 = 0x12;
const DAGCBORCODEC: u64 = 0x71;
// https://docs.rs/libipld-core/0.16.0/src/libipld_core/raw.rs.html#19
const RAWCODEC: u64 = 0x77;

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

pub fn sha256_to_cid(hash: Vec<u8>) -> Cid {
    let cid = Cid::new_v1(
        RAWCODEC,
        Multihash::<64>::wrap(SHA2_256, hash.as_slice()).unwrap(),
    );
    cid
}
