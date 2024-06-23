use crate::constants::{BASE58_MULTIBASE_PREFIX, DID_KEY_PREFIX, PLUGINS};
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
    let plugin = PLUGINS
        .into_iter()
        .find(|p| p.jwt_alg.to_string() == jwt_alg);
    if let Some(plugin) = plugin {
        let prefixed_bytes: Vec<u8> =
            [plugin.prefix.to_vec(), (plugin.compress_pubkey)(key_bytes)?].concat();

        Ok([
            BASE58_MULTIBASE_PREFIX,
            encode(Base::Base58Btc, prefixed_bytes).as_str(),
        ]
        .concat())
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
