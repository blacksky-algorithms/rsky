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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compress_decompress_roundtrip_and_length_check() {
        use secp256k1::{PublicKey, Secp256k1, SecretKey};
        let secret = SecretKey::from_slice(&[0x15u8; 32]).unwrap();
        let pubkey = PublicKey::from_secret_key(&Secp256k1::new(), &secret);
        let uncompressed = pubkey.serialize_uncompressed().to_vec();
        let compressed = compress_pubkey(uncompressed.clone()).unwrap();
        assert_eq!(compressed.len(), 33);
        assert_eq!(decompress_pubkey(compressed).unwrap(), uncompressed);
        assert!(decompress_pubkey(vec![0u8; 10]).is_err());
    }
}
