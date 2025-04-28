use std::collections::TryReserveError;

use cid::Cid;
use p256::ecdsa::signature::Verifier;
use thiserror::Error;

use crate::validator::event::{Commit, SubscribeReposCommit};
use crate::validator::types::RepoState;

const P256_DID_PREFIX: &[u8] = &[0x80, 0x24];
const K256_DID_PREFIX: &[u8] = &[0xe7, 0x01];

#[derive(Debug, Error)]
pub enum VerificationError {
    #[error("serde error: {0}")]
    Serde(#[from] serde_ipld_dagcbor::EncodeError<TryReserveError>),
    #[error("key error: {0}")]
    Key(#[from] p256::ecdsa::Error),
}

pub fn verify_commit_sig(commit: &Commit, key: &[u8; 35]) -> Result<bool, VerificationError> {
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

pub fn verify_commit_event(commit: &SubscribeReposCommit, root: Cid, prev: &RepoState) -> bool {
    if !prev.rev.older_than(&commit.rev) {
        tracing::debug!(diff = %commit.rev.timestamp() - prev.rev.timestamp(), "old rev");
        return false;
    }

    if let Some(since) = &commit.since {
        if since != &prev.rev {
            tracing::debug!(%since, "commit with miss-matching since");
            return false;
        }
    } else {
        // NOTE: some PDSs don't send this field, so we continue verifying
        tracing::trace!("missing since");
    }

    if let Some(prev_data) = &commit.prev_data {
        if prev_data != &prev.data {
            tracing::debug!(%prev_data, "commit with miss-matching prevData");
            return false;
        }
    } else {
        tracing::trace!("missing prev_data");
        return false;
    }

    let mut tree = match commit.tree(root) {
        Ok(tree) => tree,
        Err(err) => {
            tracing::debug!(%err, "unable to read MST");
            return false;
        }
    };

    // TODO: do we need to "load out all the records"?

    // TODO: once firehose format is fully shipped, remove this
    for op in &commit.ops {
        if !op.is_valid() {
            tracing::trace!(?op, "unable to invert legacy op");
            return true;
        }
    }

    // TODO: do we need to "normalize ops"?
    for (idx, op) in commit.ops.iter().enumerate() {
        match tree.invert(op) {
            Ok(inv) => {
                if !inv {
                    tracing::debug!(%idx, ?op, "unable to invert op");
                    return false;
                }
            }
            Err(err) => {
                tracing::debug!(%idx, ?op, %err, "error while inverting op");
                return false;
            }
        };
    }

    let root = match tree.root() {
        Ok(computed) => computed,
        Err(err) => {
            tracing::debug!(%err, "error while computing old root");
            return false;
        }
    };
    if let Some(prev_data) = commit.prev_data {
        if prev_data != root {
            tracing::debug!(%root, "inverted tree root didn't match prevData");
            return false;
        }
    }

    true
}
