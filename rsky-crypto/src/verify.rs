use crate::constants::PLUGINS;
use crate::did::parse_did_key;
use crate::types::VerifyOptions;
use anyhow::{bail, Result};

pub fn verify_signature(
    did_key: &String,
    data: &[u8],
    sig: &[u8],
    opts: Option<VerifyOptions>,
) -> Result<bool> {
    let parsed = parse_did_key(did_key)?;
    let plugin = PLUGINS
        .into_iter()
        .find(|p| p.jwt_alg.to_string() == parsed.jwt_alg);
    match plugin {
        None => bail!("Unsupported signature alg: {0}", parsed.jwt_alg),
        Some(plugin) => (plugin.verify_signature)(did_key, data, sig, opts),
    }
}
