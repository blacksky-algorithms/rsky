use crate::p256::plugin::P256_PLUGIN;
use crate::secp256k1::plugin::SECP256K1_PLUGIN;
use crate::types::DidKeyPlugin;

pub const BASE58_MULTIBASE_PREFIX: &str = "z";
pub const DID_KEY_PREFIX: &str = "did:key:";
pub const SECP256K1_DID_PREFIX: [u8; 2] = [0xe7, 0x01];
pub const P256_DID_PREFIX: [u8; 2] = [0x80, 0x24];
pub const P256_JWT_ALG: &str = "ES256";
pub const SECP256K1_JWT_ALG: &str = "ES256K";
pub const PLUGINS: [DidKeyPlugin; 2] = [P256_PLUGIN, SECP256K1_PLUGIN];
