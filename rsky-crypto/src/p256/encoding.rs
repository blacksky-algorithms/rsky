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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compress_decompress_roundtrip_and_length_check() {
        use p256::ecdsa::SigningKey;
        let key = SigningKey::from_slice(&[0x15u8; 32]).unwrap();
        let uncompressed = key
            .verifying_key()
            .to_encoded_point(false)
            .as_bytes()
            .to_vec();
        let compressed = compress_pubkey(uncompressed.clone()).unwrap();
        assert_eq!(compressed.len(), 33);
        assert_eq!(decompress_pubkey(compressed).unwrap(), uncompressed);
        assert!(decompress_pubkey(vec![0u8; 10]).is_err());
    }
}
