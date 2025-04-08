use std::collections::TryReserveError;

use p256::ecdsa::signature::Verifier;
use thiserror::Error;

use crate::validator::types::Commit;

const P256_DID_PREFIX: &[u8] = &[0x80, 0x24];
const K256_DID_PREFIX: &[u8] = &[0xe7, 0x01];

#[derive(Debug, Error)]
pub enum VerificationError {
    #[error("serde error: {0}")]
    Serde(#[from] serde_ipld_dagcbor::EncodeError<TryReserveError>),
    #[error("key error: {0}")]
    Key(#[from] p256::ecdsa::Error),
}

pub fn verify_commit_sig(commit: &Commit, key: [u8; 35]) -> Result<bool, VerificationError> {
    let encoded = serde_ipld_dagcbor::to_vec(commit)?;
    match &key[0..2] {
        P256_DID_PREFIX => {
            let key = p256::ecdsa::VerifyingKey::from_sec1_bytes(&key[2..])?;
            let sig = p256::ecdsa::Signature::from_slice(&commit.sig)?;
            Ok(key.verify(&encoded, &sig).is_ok())
        }
        K256_DID_PREFIX => {
            let key = k256::ecdsa::VerifyingKey::from_sec1_bytes(&key[2..])?;
            let sig = k256::ecdsa::Signature::from_slice(&commit.sig)?;
            Ok(key.verify(&encoded, &sig).is_ok())
        }
        _ => {
            unreachable!()
        }
    }
}
