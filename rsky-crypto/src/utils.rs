use crate::constants::{BASE58_MULTIBASE_PREFIX, DID_KEY_PREFIX};
use anyhow::{bail, Result};
use multibase::decode;
use multibase::Base::Base58Btc;
use secp256k1::rand::rngs::OsRng;
use secp256k1::rand::RngCore;
use secp256k1::PublicKey;
use unsigned_varint::encode::u16 as encode_varint;

pub fn extract_multikey(did: &String) -> Result<String> {
    if !did.starts_with(DID_KEY_PREFIX) {
        bail!("Incorrect prefix for did:key: {did}")
    }
    Ok(did[DID_KEY_PREFIX.len()..].to_string())
}

pub fn extract_prefixed_bytes(multikey: String) -> Result<Vec<u8>> {
    if !multikey.starts_with(BASE58_MULTIBASE_PREFIX) {
        bail!("Incorrect prefix for multikey: {multikey}")
    }
    let (_base, bytes) = decode(&multikey)?;
    Ok(bytes)
}

pub fn has_prefix(bytes: &Vec<u8>, prefix: &Vec<u8>) -> bool {
    *prefix == bytes[0..prefix.len()]
}

pub fn random_bytes(len: usize) -> Vec<u8> {
    let mut buf = vec![0u8; len];
    OsRng.fill_bytes(&mut buf);
    buf
}

/// https://github.com/gnunicorn/rust-multicodec/blob/master/src/lib.rs#L249-L260
pub fn multicodec_wrap(bytes: Vec<u8>) -> Vec<u8> {
    let mut buf = [0u8; 3];
    encode_varint(0xe7, &mut buf);
    let mut v: Vec<u8> = Vec::new();
    for b in &buf {
        v.push(*b);
        // varint uses first bit to indicate another byte follows, stop if not the case
        if *b <= 127 {
            break;
        }
    }
    v.extend(bytes);
    v
}

pub fn encode_did_key(pubkey: &PublicKey) -> String {
    let pk_compact = pubkey.serialize();
    let pk_wrapped = multicodec_wrap(pk_compact.to_vec());
    let pk_multibase = multibase::encode(Base58Btc, pk_wrapped.as_slice());
    format!("{DID_KEY_PREFIX}{pk_multibase}")
}
