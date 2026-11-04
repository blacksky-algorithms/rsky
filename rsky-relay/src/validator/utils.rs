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

fn verify_sig(msg: &[u8], sig: &[u8], key: &[u8; 35]) -> Result<bool, VerificationError> {
    match &key[0..2] {
        P256_DID_PREFIX => {
            let key = p256::ecdsa::VerifyingKey::from_sec1_bytes(&key[2..])?;
            let sig = p256::ecdsa::Signature::from_slice(sig)?;
            // atproto requires low-S: `normalize_s` returns `Some` only for a malleable high-S sig
            if sig.normalize_s().is_some() {
                tracing::debug!("rejected high-S signature");
                return Ok(false);
            }
            Ok(key.verify(msg, &sig).is_ok())
        }
        K256_DID_PREFIX => {
            let key = k256::ecdsa::VerifyingKey::from_sec1_bytes(&key[2..])?;
            let sig = k256::ecdsa::Signature::from_slice(sig)?;
            if sig.normalize_s().is_some() {
                tracing::debug!("rejected high-S signature");
                return Ok(false);
            }
            Ok(key.verify(msg, &sig).is_ok())
        }
        _ => {
            unreachable!()
        }
    }
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
            ret &= verify_sig(&encoded, sig, key)?;
        }
    }
    Ok(ret)
}

#[cfg(not(feature = "labeler"))]
pub fn verify_commit_sig(commit: &Commit, key: &[u8; 35]) -> Result<bool, VerificationError> {
    let encoded = serde_ipld_dagcbor::to_vec(commit)?;
    verify_sig(&encoded, &commit.sig, key)
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
        }
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

    use serde::Deserialize;

    use super::{K256_DID_PREFIX, P256_DID_PREFIX, verify_commit_sig, verify_sig};

    const FIXTURES: &str = include_str!("../../tests/interop/signature-fixtures.json");

    fn p256_keypair() -> (p256::ecdsa::SigningKey, [u8; 35]) {
        let signing = p256::ecdsa::SigningKey::from_slice(&[0x11; 32]).unwrap();
        let point = signing.verifying_key().to_encoded_point(true);
        let mut key = [0u8; 35];
        key[0..2].copy_from_slice(P256_DID_PREFIX);
        key[2..].copy_from_slice(point.as_bytes());
        (signing, key)
    }

    fn k256_keypair() -> (k256::ecdsa::SigningKey, [u8; 35]) {
        let signing = k256::ecdsa::SigningKey::from_slice(&[0x22; 32]).unwrap();
        let point = signing.verifying_key().to_encoded_point(true);
        let mut key = [0u8; 35];
        key[0..2].copy_from_slice(K256_DID_PREFIX);
        key[2..].copy_from_slice(point.as_bytes());
        (signing, key)
    }

    /// Deterministic (RFC 6979) low-S signature plus its high-S counterpart (`s` negated).
    fn p256_sigs(signing: &p256::ecdsa::SigningKey, msg: &[u8]) -> (Vec<u8>, Vec<u8>) {
        use p256::ecdsa::signature::Signer as _;
        let sig: p256::ecdsa::Signature = signing.sign(msg);
        let low = sig.normalize_s().unwrap_or(sig);
        let high =
            p256::ecdsa::Signature::from_scalars(*low.r().as_ref(), -*low.s().as_ref()).unwrap();
        (low.to_vec(), high.to_vec())
    }

    fn k256_sigs(signing: &k256::ecdsa::SigningKey, msg: &[u8]) -> (Vec<u8>, Vec<u8>) {
        use k256::ecdsa::signature::Signer as _;
        let sig: k256::ecdsa::Signature = signing.sign(msg);
        let low = sig.normalize_s().unwrap_or(sig);
        let high =
            k256::ecdsa::Signature::from_scalars(*low.r().as_ref(), -*low.s().as_ref()).unwrap();
        (low.to_vec(), high.to_vec())
    }

    #[test]
    fn verify_sig_rejects_high_s_p256() {
        let (signing, key) = p256_keypair();
        let (low, high) = p256_sigs(&signing, b"msg");
        assert!(matches!(verify_sig(b"msg", &low, &key), Ok(true)));
        assert!(matches!(verify_sig(b"msg", &high, &key), Ok(false)));
    }

    #[test]
    fn verify_sig_rejects_high_s_k256() {
        let (signing, key) = k256_keypair();
        let (low, high) = k256_sigs(&signing, b"msg");
        assert!(matches!(verify_sig(b"msg", &low, &key), Ok(true)));
        assert!(matches!(verify_sig(b"msg", &high, &key), Ok(false)));
    }

    #[test]
    fn verify_sig_rejects_der_encoded() {
        let (signing, key) = p256_keypair();
        let (low, _) = p256_sigs(&signing, b"msg");
        let der = p256::ecdsa::Signature::from_slice(&low).unwrap().to_der();
        assert!(verify_sig(b"msg", der.as_bytes(), &key).is_err());
    }

    #[test]
    fn verify_sig_rejects_wrong_message() {
        let (signing, key) = p256_keypair();
        let (low, _) = p256_sigs(&signing, b"msg");
        assert!(matches!(verify_sig(b"other", &low, &key), Ok(false)));
    }

    #[test]
    fn verify_sig_rejects_malformed_key() {
        let (signing, mut key) = p256_keypair();
        let (low, _) = p256_sigs(&signing, b"msg");
        key[2] = 0xff;
        assert!(verify_sig(b"msg", &low, &key).is_err());
        let (signing, mut key) = k256_keypair();
        let (low, _) = k256_sigs(&signing, b"msg");
        key[2] = 0xff;
        assert!(verify_sig(b"msg", &low, &key).is_err());
    }

    #[cfg(not(feature = "labeler"))]
    fn make_commit(sig: Vec<u8>) -> crate::validator::event::Commit {
        use rsky_common::tid::TID;

        crate::validator::event::Commit {
            did: "did:plc:test".to_owned(),
            rev: TID::new("3kqcb45gzpk2c".to_owned()).unwrap(),
            data: cid::Cid::try_from("bafyreigbtj4x7ip5legnfznufuopl4sg4knzc2cof6duas4b3q2fy6swua")
                .unwrap(),
            prev: None,
            version: 3,
            sig,
        }
    }

    #[cfg(not(feature = "labeler"))]
    #[test]
    fn verify_commit_sig_rejects_high_s_p256() {
        let (signing, key) = p256_keypair();
        let encoded = serde_ipld_dagcbor::to_vec(&make_commit(Vec::new())).unwrap();
        let (low, high) = p256_sigs(&signing, &encoded);
        assert!(matches!(verify_commit_sig(&make_commit(low), &key), Ok(true)));
        assert!(matches!(verify_commit_sig(&make_commit(high), &key), Ok(false)));
    }

    #[cfg(not(feature = "labeler"))]
    #[test]
    fn verify_commit_sig_rejects_high_s_k256() {
        let (signing, key) = k256_keypair();
        let encoded = serde_ipld_dagcbor::to_vec(&make_commit(Vec::new())).unwrap();
        let (low, high) = k256_sigs(&signing, &encoded);
        assert!(matches!(verify_commit_sig(&make_commit(low), &key), Ok(true)));
        assert!(matches!(verify_commit_sig(&make_commit(high), &key), Ok(false)));
    }

    #[cfg(feature = "labeler")]
    fn make_label(sig: Option<Vec<u8>>) -> crate::validator::event::SubscribeLabel {
        crate::validator::event::SubscribeLabel {
            ver: Some(1),
            src: "did:plc:test".to_owned(),
            uri: "at://did:plc:test/app.bsky.feed.post/3lr4pmliavk2l".to_owned(),
            cid: None,
            val: "spam".to_owned(),
            neg: None,
            cts: "2026-01-12T19:45:23.307Z".to_owned(),
            cts_dt: chrono::DateTime::parse_from_rfc3339("2026-01-12T19:45:23.307Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
            exp: None,
            sig,
        }
    }

    #[cfg(feature = "labeler")]
    #[test]
    fn verify_commit_sig_rejects_high_s_p256() {
        let (signing, key) = p256_keypair();
        let encoded = serde_ipld_dagcbor::to_vec(&make_label(None)).unwrap();
        let (low, high) = p256_sigs(&signing, &encoded);
        assert!(matches!(verify_commit_sig(&[make_label(Some(low))], &key), Ok(true)));
        assert!(matches!(verify_commit_sig(&[make_label(Some(high))], &key), Ok(false)));
    }

    #[cfg(feature = "labeler")]
    #[test]
    fn verify_commit_sig_rejects_high_s_k256() {
        let (signing, key) = k256_keypair();
        let encoded = serde_ipld_dagcbor::to_vec(&make_label(None)).unwrap();
        let (low, high) = k256_sigs(&signing, &encoded);
        assert!(matches!(verify_commit_sig(&[make_label(Some(low))], &key), Ok(true)));
        assert!(matches!(verify_commit_sig(&[make_label(Some(high))], &key), Ok(false)));
    }

    #[cfg(feature = "labeler")]
    #[test]
    fn verify_commit_sig_ignores_unsigned_labels() {
        let (_, key) = p256_keypair();
        assert!(matches!(verify_commit_sig(&[make_label(None)], &key), Ok(true)));
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Fixture {
        comment: String,
        message_base64: String,
        public_key_did: String,
        signature_base64: String,
        valid_signature: bool,
        tags: Vec<String>,
    }

    /// Fixtures mix the base64 and base64url alphabets; multibase `m` is unpadded base64.
    fn b64(input: &str) -> Vec<u8> {
        let normalized = format!("m{}", input.replace('-', "+").replace('_', "/"));
        multibase::decode(normalized).unwrap().1
    }

    fn did_key(did: &str) -> [u8; 35] {
        multibase::decode(did.trim_start_matches("did:key:")).unwrap().1.try_into().unwrap()
    }

    #[test]
    fn atproto_interop_signature_fixtures() {
        let fixtures: Vec<Fixture> = serde_json::from_str(FIXTURES).unwrap();
        assert_eq!(fixtures.len(), 6);

        let mut valid = 0;
        let mut high_s = 0;
        let mut der = 0;
        for fixture in &fixtures {
            let msg = b64(&fixture.message_base64);
            let sig = b64(&fixture.signature_base64);
            let key = did_key(&fixture.public_key_did);
            let got = verify_sig(&msg, &sig, &key);
            assert_eq!(matches!(got, Ok(true)), fixture.valid_signature, "{}", fixture.comment);
            if fixture.tags.iter().any(|tag| tag == "high-s") {
                high_s += 1;
                assert!(matches!(got, Ok(false)), "{}", fixture.comment);
            } else if fixture.tags.iter().any(|tag| tag == "der-encoded") {
                der += 1;
                assert!(got.is_err(), "{}", fixture.comment);
            } else {
                valid += 1;
                assert!(fixture.valid_signature, "{}", fixture.comment);
            }
        }
        assert_eq!((valid, high_s, der), (2, 2, 2));
    }
}
