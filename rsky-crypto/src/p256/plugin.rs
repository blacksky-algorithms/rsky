use crate::constants::{P256_DID_PREFIX, P256_JWT_ALG};
use crate::p256::encoding::{compress_pubkey, decompress_pubkey};
use crate::p256::operations::verify_did_sig;
use crate::types::DidKeyPlugin;

pub const P256_PLUGIN: DidKeyPlugin = DidKeyPlugin {
    prefix: P256_DID_PREFIX,
    jwt_alg: P256_JWT_ALG,
    compress_pubkey,
    decompress_pubkey,
    verify_signature: verify_did_sig,
};
