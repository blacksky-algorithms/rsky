use crate::constants::{DID_KEY_PREFIX, PLUGINS};
use crate::utils::{extract_multikey, extract_prefixed_bytes, has_prefix};
use anyhow::{bail, Result};
use multibase::{encode, Base};

#[derive(Clone)]
pub struct ParsedMultikey {
    pub jwt_alg: String,
    pub key_bytes: Vec<u8>,
}

pub fn parse_multikey(multikey: String) -> Result<ParsedMultikey> {
    let prefixed_bytes = extract_prefixed_bytes(multikey)?;
    let plugin = PLUGINS
        .into_iter()
        .find(|p| has_prefix(&prefixed_bytes, &p.prefix.to_vec()));
    if let Some(plugin) = plugin {
        let key_bytes = (plugin.decompress_pubkey)(prefixed_bytes[plugin.prefix.len()..].to_vec())?;
        Ok(ParsedMultikey {
            jwt_alg: plugin.jwt_alg.to_string(),
            key_bytes,
        })
    } else {
        bail!("Unsupported key type")
    }
}

pub fn format_multikey(jwt_alg: String, key_bytes: Vec<u8>) -> Result<String> {
    let plugin = PLUGINS.into_iter().find(|p| p.jwt_alg == jwt_alg);
    if let Some(plugin) = plugin {
        let prefixed_bytes: Vec<u8> =
            [plugin.prefix.to_vec(), (plugin.compress_pubkey)(key_bytes)?].concat();

        // multibase::encode already emits the base58btc prefix character.
        Ok(encode(Base::Base58Btc, prefixed_bytes))
    } else {
        bail!("Unsupported key type")
    }
}

pub fn parse_did_key(did: &String) -> Result<ParsedMultikey> {
    let multikey = extract_multikey(did)?;
    parse_multikey(multikey)
}

pub fn format_did_key(jwt_alg: String, key_bytes: Vec<u8>) -> Result<String> {
    Ok([
        DID_KEY_PREFIX,
        format_multikey(jwt_alg, key_bytes)?.as_str(),
    ]
    .concat())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::{P256_JWT_ALG, SECP256K1_JWT_ALG};

    #[test]
    fn format_parse_roundtrip_secp256k1() {
        use secp256k1::{PublicKey, Secp256k1, SecretKey};
        let secret = SecretKey::from_slice(&[0x24u8; 32]).unwrap();
        let pubkey = PublicKey::from_secret_key(&Secp256k1::new(), &secret);
        let did = format_did_key(
            SECP256K1_JWT_ALG.to_string(),
            pubkey.serialize_uncompressed().to_vec(),
        )
        .unwrap();
        assert!(did.starts_with("did:key:z"));
        assert!(!did.starts_with("did:key:zz"));
        let parsed = parse_did_key(&did).unwrap();
        assert_eq!(parsed.jwt_alg, SECP256K1_JWT_ALG);
        // Round-trips to the identical did:key via the compress path.
        assert_eq!(
            format_did_key(parsed.jwt_alg, parsed.key_bytes).unwrap(),
            did
        );
        // Matches the independent encoder used across the workspace.
        assert_eq!(crate::utils::encode_did_key(&pubkey), did);
    }

    #[test]
    fn unknown_prefix_and_alg_rejected() {
        // A valid multibase payload whose multicodec prefix is not a known curve.
        let bogus = multibase::encode(multibase::Base::Base58Btc, [0xAA, 0xAA, 1, 2, 3]);
        assert!(parse_multikey(bogus).is_err());
        assert!(format_multikey("EdDSA".to_string(), vec![0u8; 32]).is_err());
    }

    #[test]
    fn format_parse_roundtrip_p256() {
        use p256::ecdsa::SigningKey;
        let signing_key = SigningKey::from_slice(&[0x24u8; 32]).unwrap();
        let sec1 = signing_key
            .verifying_key()
            .to_encoded_point(false)
            .as_bytes()
            .to_vec();
        let did = format_did_key(P256_JWT_ALG.to_string(), sec1).unwrap();
        assert!(did.starts_with("did:key:z"));
        assert!(!did.starts_with("did:key:zz"));
        let parsed = parse_did_key(&did).unwrap();
        assert_eq!(parsed.jwt_alg, P256_JWT_ALG);
    }
}
