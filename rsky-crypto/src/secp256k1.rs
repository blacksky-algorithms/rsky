use anyhow::Result;
use secp256k1::PublicKey;

pub fn compress_pubkey(pubkey_bytes: Vec<u8>) -> Result<Vec<u8>> {
    let point = PublicKey::from_slice(pubkey_bytes.as_slice())?.serialize();
    Ok(point.to_vec())
}
