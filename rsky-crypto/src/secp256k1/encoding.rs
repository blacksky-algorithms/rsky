use anyhow::{bail, Result};
use secp256k1::PublicKey;

pub fn compress_pubkey(pubkey_bytes: Vec<u8>) -> Result<Vec<u8>> {
    let point = PublicKey::from_slice(pubkey_bytes.as_slice())?.serialize();
    Ok(point.to_vec())
}

pub fn decompress_pubkey(compressed: Vec<u8>) -> Result<Vec<u8>> {
    if compressed.len() != 33 {
        bail!("Expected 33 byte compress pubkey")
    }
    let point = PublicKey::from_slice(compressed.as_slice())?.serialize_uncompressed();
    Ok(point.to_vec())
}
