use anyhow::{bail, Result};
use p256::ecdsa::VerifyingKey;

pub fn compress_pubkey(pubkey_bytes: Vec<u8>) -> Result<Vec<u8>> {
    let point = VerifyingKey::from_sec1_bytes(pubkey_bytes.as_slice())?.to_encoded_point(true);
    Ok(point.as_bytes().to_vec())
}

pub fn decompress_pubkey(compressed: Vec<u8>) -> Result<Vec<u8>> {
    if compressed.len() != 33 {
        bail!("Expected 33 byte compress pubkey")
    }
    let point = VerifyingKey::from_sec1_bytes(compressed.as_slice())?.to_encoded_point(false);
    Ok(point.as_bytes().to_vec())
}
