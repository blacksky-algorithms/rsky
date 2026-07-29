use anyhow::Result;
use rsky_crypto::constants::{P256_JWT_ALG, SECP256K1_JWT_ALG};
use rsky_crypto::did::{format_did_key, parse_multikey};
use rsky_crypto::multibase::multibase_to_bytes;

#[derive(Clone)]
pub struct VerificationMaterial {
    pub r#type: String,
    pub public_key_multibase: String,
}

pub fn get_did_key_from_multibase(key: VerificationMaterial) -> Result<Option<String>> {
    let key_bytes = multibase_to_bytes(key.public_key_multibase.clone())?;
    let did_key = match key.r#type.as_str() {
        "EcdsaSecp256r1VerificationKey2019" => {
            Some(format_did_key(P256_JWT_ALG.to_string(), key_bytes)?)
        }
        "EcdsaSecp256k1VerificationKey2019" => {
            Some(format_did_key(SECP256K1_JWT_ALG.to_string(), key_bytes)?)
        }
        "Multikey" => {
            let parsed = parse_multikey(key.public_key_multibase)?;
            Some(format_did_key(parsed.jwt_alg, parsed.key_bytes)?)
        }
        _ => None,
    };
    Ok(did_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use secp256k1::{PublicKey, Secp256k1, SecretKey};

    fn test_pubkey() -> PublicKey {
        let secret = SecretKey::from_slice(&[0x33u8; 32]).unwrap();
        PublicKey::from_secret_key(&Secp256k1::new(), &secret)
    }

    #[test]
    fn legacy_verification_key_type_resolves_to_did_key() {
        let pubkey = test_pubkey();
        let expected = rsky_crypto::utils::encode_did_key(&pubkey);
        // Legacy 2019 types carry the bare key bytes multibase-encoded,
        // without a multicodec prefix.
        let material = VerificationMaterial {
            r#type: "EcdsaSecp256k1VerificationKey2019".to_string(),
            public_key_multibase: multibase::encode(multibase::Base::Base58Btc, pubkey.serialize()),
        };
        let did_key = get_did_key_from_multibase(material).unwrap().unwrap();
        assert_eq!(did_key, expected);
    }

    #[test]
    fn legacy_p256_verification_key_type_resolves_to_did_key() {
        let material = VerificationMaterial {
            r#type: "EcdsaSecp256r1VerificationKey2019".to_string(),
            // Compressed P-256 generator point.
            public_key_multibase: multibase::encode(
                multibase::Base::Base58Btc,
                hex::decode("036b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296")
                    .unwrap(),
            ),
        };
        let did_key = get_did_key_from_multibase(material).unwrap().unwrap();
        assert!(did_key.starts_with("did:key:z"));
        assert_eq!(
            rsky_crypto::did::parse_did_key(&did_key).unwrap().jwt_alg,
            P256_JWT_ALG
        );
    }

    #[test]
    fn multikey_type_resolves_to_did_key() {
        let pubkey = test_pubkey();
        let expected = rsky_crypto::utils::encode_did_key(&pubkey);
        // Multikey carries the multicodec-prefixed key; reuse the encoder and
        // strip the did:key: prefix to get the multikey form.
        let multikey = expected.strip_prefix("did:key:").unwrap().to_string();
        let material = VerificationMaterial {
            r#type: "Multikey".to_string(),
            public_key_multibase: multikey,
        };
        let did_key = get_did_key_from_multibase(material).unwrap().unwrap();
        assert_eq!(did_key, expected);
    }

    #[test]
    fn unknown_type_resolves_to_none() {
        let material = VerificationMaterial {
            r#type: "Ed25519VerificationKey2020".to_string(),
            public_key_multibase: "zunused".to_string(),
        };
        assert!(get_did_key_from_multibase(material).unwrap().is_none());
    }
}
