//! subscribeRepos decoding that tolerates a missing `tooBig` on `#commit`
//!
//! for bridgy <3
//!
//! plugs a lenient decoder into jacquard's subscription trait: `#commit` body
//! decodes through a mirror struct that defaults `tooBig` to `false` when
//! absent. every other message type delegates back to jacquard's

use std::collections::BTreeMap;

use jacquard_api::com_atproto::sync::subscribe_repos::{
    Commit, RepoOp, SubscribeReposError, SubscribeReposMessage,
};
use jacquard_common::deps::bytes::Bytes;
use jacquard_common::deps::codegen::serde_ipld_dagcbor;
use jacquard_common::deps::smol_str::SmolStr;
use jacquard_common::error::DecodeError;
use jacquard_common::types::cid::CidLink;
use jacquard_common::types::string::{Datetime, Did, Tid};
use jacquard_common::types::value::Data;
use jacquard_common::xrpc::subscription::parse_event_header;
use jacquard_common::xrpc::{MessageEncoding, SubscriptionResp, XrpcSubscription};
use jacquard_common::{BosStr, DefaultStr};
use serde::Deserialize;

/// `com.atproto.sync.subscribeRepos` params whose stream decodes tolerantly
///
/// mirrors jacquard's `SubscribeRepos` params; only the associated `Stream`
/// (and so the `#commit` decoding) differs.
#[derive(Debug, serde::Serialize)]
pub(super) struct TolerantSubscribeRepos {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<i64>,
}

impl XrpcSubscription for TolerantSubscribeRepos {
    const NSID: &'static str = "com.atproto.sync.subscribeRepos";
    const ENCODING: MessageEncoding = MessageEncoding::DagCbor;
    type Stream = TolerantSubscribeReposStream;
}

/// stream marker: like jacquard's `SubscribeReposStream` with lenient `#commit`
pub(super) struct TolerantSubscribeReposStream;

impl SubscriptionResp for TolerantSubscribeReposStream {
    const NSID: &'static str = "com.atproto.sync.subscribeRepos";
    const ENCODING: MessageEncoding = MessageEncoding::DagCbor;
    type Message<S: BosStr> = SubscribeReposMessage<S>;
    type Error = SubscribeReposError;

    fn decode_message<'de, S>(bytes: &'de [u8]) -> Result<Self::Message<S>, DecodeError>
    where
        S: BosStr + Deserialize<'de>,
        Self::Message<S>: Deserialize<'de>,
    {
        let (header, body) = parse_event_header(bytes)?;
        if header.t == "#commit" {
            let commit: TolerantCommit<S> = serde_ipld_dagcbor::from_slice(body)?;
            return Ok(SubscribeReposMessage::Commit(Box::new(commit.into())));
        }
        // non-commit types have no tolerance to apply: stock decode
        // (re-parses the tiny header; not worth plumbing around)
        SubscribeReposMessage::decode_framed(bytes)
    }
}

/// same as jacquard's `Commit`, except `tooBig` gets `#[serde(default)]`
///
/// keep synchronized with the pinned jacquard-api version (round-trip tests
/// should fail if it changes)
#[derive(Deserialize)]
#[serde(
    rename_all = "camelCase",
    bound(deserialize = "S: Deserialize<'de> + BosStr")
)]
struct TolerantCommit<S: BosStr = DefaultStr> {
    blobs: Vec<CidLink<S>>,
    #[serde(with = "jacquard_common::serde_bytes_helper")]
    blocks: Bytes,
    commit: CidLink<S>,
    ops: Vec<RepoOp<S>>,
    prev_data: Option<CidLink<S>>,
    rebase: bool,
    repo: Did<S>,
    rev: Tid,
    seq: i64,
    since: Option<Tid>,
    time: Datetime,
    /// the point of all this: default to false when the field is missing
    #[serde(default)]
    too_big: bool,
    #[serde(flatten, default)]
    extra_data: Option<BTreeMap<SmolStr, Data<S>>>,
}

impl<S: BosStr> From<TolerantCommit<S>> for Commit<S> {
    fn from(c: TolerantCommit<S>) -> Self {
        Commit {
            blobs: c.blobs,
            blocks: c.blocks,
            commit: c.commit,
            ops: c.ops,
            prev_data: c.prev_data,
            rebase: c.rebase,
            repo: c.repo,
            rev: c.rev,
            seq: c.seq,
            since: c.since,
            time: c.time,
            too_big: c.too_big,
            extra_data: c.extra_data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jacquard_common::types::cid::{Cid as JacquardCid, IpldCid};

    // a real dag-cbor CIDv1 so CidLink round-trips as a proper tag-42 link
    const CID: &str = "bafyreieebzlwyqijbjyzfpjhzicsniw3h3zc3hpen2m66jgg543kpnpg4e";

    /// a `Cid::Ipld`-variant link: the variant that CBOR-serializes as tag 42
    /// (the `Str` variant writes a plain text string, which decode rejects)
    fn cid_link() -> CidLink {
        CidLink(JacquardCid::ipld(
            IpldCid::try_from(CID).expect("valid cid"),
        ))
    }
    const DID: &str = "did:plc:hdhoaan3xa3jiuq4fg4mefid";
    const REV: &str = "3jzfcijpj2z2a";
    const TIME: &str = "2026-07-27T00:00:00.000Z";

    /// serialize-only twin producing the exact wire shape of a `#commit` body
    /// -- plain strings serialize identically to jacquard's validating types
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct BodyShape {
        blobs: Vec<CidLink>,
        #[serde(with = "jacquard_common::serde_bytes_helper")]
        blocks: Bytes,
        commit: CidLink,
        ops: Vec<()>,
        rebase: bool,
        repo: &'static str,
        rev: &'static str,
        seq: i64,
        time: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        too_big: Option<bool>,
    }

    fn body(too_big: Option<bool>) -> Vec<u8> {
        serde_ipld_dagcbor::to_vec(&BodyShape {
            blobs: vec![],
            blocks: Bytes::from_static(b"not-a-real-car"),
            commit: cid_link(),
            ops: vec![],
            rebase: false,
            repo: DID,
            rev: REV,
            seq: 42,
            time: TIME,
            too_big,
        })
        .expect("serialize test body")
    }

    #[derive(serde::Serialize)]
    struct HeaderShape {
        op: i64,
        t: &'static str,
    }

    fn frame(t: &'static str, body: &[u8]) -> Vec<u8> {
        let mut f = serde_ipld_dagcbor::to_vec(&HeaderShape { op: 1, t }).expect("header");
        f.extend_from_slice(body);
        f
    }

    fn decode(frame: &[u8]) -> SubscribeReposMessage {
        TolerantSubscribeReposStream::decode_message::<DefaultStr>(frame).expect("decode")
    }

    #[test]
    fn missing_toobig_defaults_to_false() {
        let SubscribeReposMessage::Commit(c) = decode(&frame("#commit", &body(None))) else {
            panic!("expected #commit");
        };
        assert!(!c.too_big);
        assert_eq!(c.seq, 42);
        assert_eq!(c.repo.as_str(), DID);
        assert_eq!(&*c.blocks, b"not-a-real-car");
    }

    #[test]
    fn present_toobig_is_respected() {
        let SubscribeReposMessage::Commit(c) = decode(&frame("#commit", &body(Some(true)))) else {
            panic!("expected #commit");
        };
        assert!(c.too_big);
    }

    #[test]
    fn compliant_commit_matches_stock_decode() {
        // mirror-struct drift guard: with tooBig present, the tolerant path
        // must produce exactly what jacquard's own decoder does
        let f = frame("#commit", &body(Some(false)));
        let tolerant = decode(&f);
        let stock = SubscribeReposMessage::<DefaultStr>::decode_framed(&f).expect("stock decode");
        assert_eq!(tolerant, stock);
    }

    #[test]
    fn non_commit_messages_delegate_to_stock_decode() {
        #[derive(serde::Serialize)]
        struct AccountShape {
            active: bool,
            did: &'static str,
            seq: i64,
            time: &'static str,
        }
        let body = serde_ipld_dagcbor::to_vec(&AccountShape {
            active: true,
            did: DID,
            seq: 43,
            time: TIME,
        })
        .expect("serialize account");
        let SubscribeReposMessage::Account(a) = decode(&frame("#account", &body)) else {
            panic!("expected #account");
        };
        assert_eq!(a.seq, 43);
        assert!(a.active);
    }
}
