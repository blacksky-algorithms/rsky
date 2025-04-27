use std::collections::TryReserveError;

use cid::Cid;
use p256::ecdsa::signature::Verifier;
use thiserror::Error;

use rsky_common::tid::TID;

use crate::validator::event::{Commit, SubscribeReposEvent};
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

pub fn verify_commit_msg(
    event: &SubscribeReposEvent, rev: &TID, root: Cid, prev: &RepoState,
) -> bool {
    let did = event.did();
    if !prev.rev.older_than(rev) {
        tracing::debug!(
            "[{did}] old msg: {rev} -> {} ({})",
            prev.rev,
            rev.timestamp() - prev.rev.timestamp()
        );
        return false;
    }

    if let SubscribeReposEvent::Commit(commit) = &event {
        if let Some(rev) = &commit.since {
            if rev != &prev.rev.0 {
                tracing::debug!("[{did}] prev_rev mismatch: {rev} (expected: {})", prev.rev);
                return false;
            }
        } else {
            // NOTE: some PDSs don't send this field, so we continue verifying
            tracing::trace!("[{did}] missing since");
        }

        if let Some(data) = &commit.prev_data {
            if data != &prev.data {
                tracing::debug!("[{did}] prev_data mismatch");
                return false;
            }
        } else {
            tracing::trace!("[{did}] missing prev_data");
            return false;
        }

        let Ok(mut tree) = commit.tree(root) else {
            tracing::debug!("[{}] unable to read MST", commit.repo);
            return false;
        };
        // TODO: check that commit CID matches root? re-compute?

        // TODO: do we need to "load out all the records"?

        for op in &commit.ops {
            if !op.is_valid() {
                tracing::trace!("[{}] unable to invert legacy op", commit.repo);
                // TODO: once firehose format is fully shipped, remove this
                return true;
            }
        }

        // TODO: do we need to "normalize ops"?

        for op in &commit.ops {
            match tree.invert(op) {
                Ok(inv) => {
                    if !inv {
                        return false;
                    }
                }
                Err(err) => {
                    tracing::trace!("[{}] error while inverting: {err} ({op:?})", commit.repo);
                    return false;
                }
            };
        }

        let found = match tree.root() {
            Ok(found) => found,
            Err(err) => {
                tracing::trace!("[{}] error while computing root: {err}", commit.repo);
                return false;
            }
        };
        if let Some(expected) = commit.prev_data {
            if expected != found {
                tracing::debug!("inverted tree root mismatch: {found} ({expected})");
                return false;
            }
        }
    }

    true
}
