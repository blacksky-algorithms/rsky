use crate::constants::{SECP256K1_DID_PREFIX, SECP256K1_JWT_ALG};
use crate::secp256k1::encoding::{compress_pubkey, decompress_pubkey};
use crate::secp256k1::operations::verify_did_sig;
use crate::types::DidKeyPlugin;

pub const SECP256K1_PLUGIN: DidKeyPlugin = DidKeyPlugin {
    prefix: SECP256K1_DID_PREFIX,
    jwt_alg: SECP256K1_JWT_ALG,
    compress_pubkey,
    decompress_pubkey,
    verify_signature: verify_did_sig,
};
