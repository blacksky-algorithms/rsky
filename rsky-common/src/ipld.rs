use anyhow::Result;
use lexicon_cid::Cid;
use multihash::Multihash;
use serde::Serialize;
use sha2::{Digest, Sha256};

const SHA2_256: u64 = 0x12;
const DAGCBORCODEC: u64 = 0x71;
const RAWCODEC: u64 = 0x55;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_to_cid_uses_raw_codec() {
        let hash = Sha256::digest(b"").to_vec();
        let cid = sha256_to_cid(hash);
        assert_eq!(cid.codec(), 0x55);
        // CIDv1 raw sha2-256 of empty input
        assert_eq!(
            cid.to_string(),
            "bafkreihdwdcefgh4dqkjv67uzcmw7ojee6xedzdetojuzjevtenxquvyku"
        );
    }

    #[test]
    fn cid_for_cbor_uses_dag_cbor_codec() {
        let cid = cid_for_cbor(&serde_json::json!({"a": 1})).unwrap();
        assert_eq!(cid.codec(), 0x71);
    }
}
