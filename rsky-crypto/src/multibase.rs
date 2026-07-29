use anyhow::Result;

/// Decode a multibase-prefixed string to its raw bytes.
pub fn multibase_to_bytes(mb: String) -> Result<Vec<u8>> {
    let (_base, bytes) = multibase::decode(&mb)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use multibase::Base;

    #[test]
    fn decodes_each_supported_base() {
        let payload = b"\x04\x01\x02\x03\xff".to_vec();
        for base in [
            Base::Base16Lower,
            Base::Base16Upper,
            Base::Base32Lower,
            Base::Base32Upper,
            Base::Base58Btc,
            Base::Base64,
            Base::Base64Url,
            Base::Base64UrlPad,
        ] {
            let encoded = multibase::encode(base, &payload);
            assert_eq!(multibase_to_bytes(encoded).unwrap(), payload);
        }
    }

    #[test]
    fn rejects_empty_and_unknown_prefix() {
        assert!(multibase_to_bytes(String::new()).is_err());
        assert!(multibase_to_bytes("!notmultibase".to_string()).is_err());
    }
}
