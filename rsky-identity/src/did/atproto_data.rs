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
