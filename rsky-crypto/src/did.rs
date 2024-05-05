use crate::constants::{BASE58_MULTIBASE_PREFIX, DID_KEY_PREFIX, SECP256K1_DID_PREFIX};
use crate::secp256k1::compress_pubkey;
use anyhow::{bail, Result};
use multibase::{encode, Base};

#[derive(Clone)]
pub struct ParsedMultikey {
    pub jwt_alg: String,
    pub key_bytes: Vec<u8>,
}

pub fn parse_multikey(multikey: String) -> Result<ParsedMultikey> {
    todo!()
}

pub fn format_multikey(key_bytes: Vec<u8>) -> Result<String> {
    let prefixed_bytes: Vec<u8> =
        [SECP256K1_DID_PREFIX.to_vec(), compress_pubkey(key_bytes)?].concat();

    Ok([
        BASE58_MULTIBASE_PREFIX,
        encode(Base::Base58Btc, prefixed_bytes).as_str(),
    ]
    .concat())
}

pub fn format_did_key(key_bytes: Vec<u8>) -> Result<String> {
    Ok([DID_KEY_PREFIX, format_multikey(key_bytes)?.as_str()].concat())
}
