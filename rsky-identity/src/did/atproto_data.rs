use anyhow::{bail, Result};
use rsky_crypto::did::format_did_key;
use rsky_crypto::multibase::multibase_to_bytes;

#[derive(Clone)]
pub struct VerificationMaterial {
    pub r#type: String,
    pub public_key_multibase: String,
}

pub fn get_did_key_from_multibase(key: VerificationMaterial) -> Result<Option<String>> {
    let key_bytes = multibase_to_bytes(key.public_key_multibase)?;
    let did_key = match key.r#type.as_str() {
        "EcdsaSecp256k1VerificationKey2019" => Some(format_did_key(key_bytes)?),
        "EcdsaSecp256r1VerificationKey2019" => unimplemented!(),
        "Multikey" => unimplemented!(),
        _ => None,
    };
    Ok(did_key)
}
