use std::collections::TryReserveError;

#[cfg(not(feature = "labeler"))]
use cid::Cid;
use p256::ecdsa::signature::Verifier;
use thiserror::Error;

#[cfg(feature = "labeler")]
use crate::validator::event::SubscribeLabel;
#[cfg(not(feature = "labeler"))]
use crate::validator::event::{Commit, SubscribeReposCommit};
#[cfg(not(feature = "labeler"))]
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

#[cfg(feature = "labeler")]
pub fn verify_commit_sig(
    labels: &[SubscribeLabel], key: &[u8; 35],
) -> Result<bool, VerificationError> {
    let mut ret = true;
    for label in labels {
        if let Some(sig) = &label.sig {
            let mut label = label.clone();
            label.sig = None;
            let encoded = serde_ipld_dagcbor::to_vec(&label)?;
            match &key[0..2] {
                P256_DID_PREFIX => {
                    let key = p256::ecdsa::VerifyingKey::from_sec1_bytes(&key[2..])?;
                    let sig = p256::ecdsa::Signature::from_slice(sig)?;
                    ret &= key.verify(&encoded, &sig).is_ok();
                }
                K256_DID_PREFIX => {
                    let key = k256::ecdsa::VerifyingKey::from_sec1_bytes(&key[2..])?;
                    let sig = k256::ecdsa::Signature::from_slice(sig)?;
                    ret &= key.verify(&encoded, &sig).is_ok();
                }
                _ => {
                    unreachable!()
                }
            }
        }
    }
    Ok(ret)
}

#[cfg(not(feature = "labeler"))]
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

#[cfg(not(feature = "labeler"))]
pub fn verify_commit_event(commit: &SubscribeReposCommit, root: Cid, prev: &RepoState) -> bool {
    if !prev.rev.older_than(&commit.rev) {
        tracing::debug!(diff = %commit.rev.timestamp() - prev.rev.timestamp(), "old rev");
        return false;
    }

    if let Some(since) = &commit.since {
        if since != &prev.rev {
            // TODO: change back to debug
            tracing::trace!(%since, "commit with miss-matching since");
        }
    } else {
        // NOTE: some PDSs don't send this field, so we continue verifying
        tracing::trace!("missing since");
    }

    if let Some(prev_data) = &commit.prev_data {
        if prev_data != &prev.data {
            // TODO: change back to debug
            tracing::trace!(%prev_data, "commit with miss-matching prevData");
        }
    } else {
        tracing::trace!("missing prev_data");
        return false;
    }

    let mut tree = match commit.tree(root) {
        Ok(tree) => tree,
        Err(err) => {
            if commit.ops.is_empty() && prev.data == root {
                tracing::debug!(%err, "empty #commit");
            } else {
                tracing::debug!(%err, ops = %commit.ops.len(), "unable to read MST");
            }
            return true;
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
                    return true;
                }
            }
            Err(err) => {
                tracing::debug!(%idx, ?op, %err, "error while inverting op");
                return true;
            }
        };
    }

    let root = match tree.root() {
        Ok(computed) => computed,
        Err(err) => {
            tracing::debug!(%err, "error while computing old root");
            return true;
        }
    };
    if let Some(prev_data) = commit.prev_data {
        if prev_data != root {
            tracing::debug!(%root, "inverted tree root didn't match prevData");
            return true;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "labeler")]
    #[test]
    fn verify_commit() {
        use crate::validator::event::SubscribeReposEvent;
        use crate::validator::utils::verify_commit_sig;

        const KEY: &[u8; 35] = b"\xe7\x01\x03e\xac\xdb.\x10\x1c\xb5O_\x1d\x95\x13T\x90$\x12pM\x07^^w~\xae\x94\xc2\xfe\xe4\xba\xb0\xf3\x91";
        const MSG: &[u8] = b"\xa2atg#labelsbop\x01\xa2cseq\x1a\x00\xf2\xbb\xdfflabels\x81\xa7ccidx;bafyreiapddjgxnyaogx2gvakuawukls5rr2hdwbkrjb4nwjffwpkb4734mcctsx\x182025-06-08T20:34:56.000ZcsigX@\x99\xecj!\xd8_\x8dg\xb5G\t\x83\xdf\x90\xc64K~6%\r\x8f\xe2{\xe5r\xbf<8\x16\x0fgX9\x84n\x05j\x9d\xf0\x83\xc4b\xbe\xd7\x97\xfa3)\xedE\xb0I\xa1\xfd\xf5\x17\xd8\xccPA\xa4k\x81csrcx did:plc:ar7c4by46qjdydhdevvrndaccurixFat://did:plc:ubfdrhs4desxu3u4osnulj6b/app.bsky.feed.post/3lr4pmliavk2lcvaldporncver\x01";

        let Ok(Some(SubscribeReposEvent::Labels(labels))) = SubscribeReposEvent::parse(MSG) else {
            unreachable!()
        };
        assert!(verify_commit_sig(&labels.labels, KEY).unwrap_or(false));
    }
}
