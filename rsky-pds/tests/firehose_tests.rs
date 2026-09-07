use futures::StreamExt;
use lexicon_cid::Cid;
use rocket::http::{ContentType, Header, Status};
use rocket::local::asynchronous::Client;
use rsky_common::time::from_str_to_utc;
use rsky_lexicon::com::atproto::server::CreateInviteCodeOutput;
use rsky_lexicon::com::atproto::sync::{
    AccountStatus, SubscribeReposAccount, SubscribeReposCommit, SubscribeReposCommitOperation,
    SubscribeReposIdentity, SubscribeReposSync,
};
use rsky_pds::config::ServerConfig;
use rsky_pds::sequencer::events::{CommitEvt, CommitEvtOpAction, IdentityEvt, SeqEvt, SyncEvt};
use rsky_pds::sequencer::outbox::{Outbox, OutboxOpts};
use rsky_pds::sequencer::{RequestSeqRangeOpts, Sequencer};
use rsky_pds::xrpc_server::stream::frames::{Frame, MessageFrame, MessageFrameOpts};
use rsky_pds::xrpc_server::stream::types::InfoFrameBody;
use rsky_pds::SharedSequencer;
use rsky_repo::car::read_car_with_root;
use rsky_repo::types::Commit;
use serde_cbor::Value as CborValue;
use serde_json::{json, Value};
use std::str::FromStr;

mod common;

const COLLECTION: &str = "app.bsky.feed.post";

/// Pinned so nothing in the suite depends on the wall clock.
const FIXED_TIME: &str = "2026-01-12T19:45:23.307Z";

/// An active account with a live repo, plus an access token for it. Every
/// write in these tests goes through the real XRPC endpoints, so the sequencer
/// sees exactly what a client would produce.
struct Harness {
    _dir: tempfile::TempDir,
    client: Client,
    did: String,
    handle: String,
    token: String,
}

impl Harness {
    /// Creates the account without a caller-supplied did, which is the path
    /// that leaves the account active and sequences its creation events.
    async fn new() -> Self {
        let (dir, client) = common::get_client().await;
        let domain = client
            .rocket()
            .state::<ServerConfig>()
            .unwrap()
            .identity
            .service_handle_domains
            .first()
            .unwrap()
            .clone();
        let handle = format!("foo{domain}");
        let invite = client
            .post("/xrpc/com.atproto.server.createInviteCode")
            .header(ContentType::JSON)
            .header(Header::new("Authorization", common::get_admin_token()))
            .body(json!({ "useCount": 1 }).to_string())
            .dispatch()
            .await
            .into_json::<CreateInviteCodeOutput>()
            .await
            .unwrap()
            .code;
        let response = client
            .post("/xrpc/com.atproto.server.createAccount")
            .header(ContentType::JSON)
            .body(
                json!({
                    "email": "firehose@example.com",
                    "handle": handle,
                    "password": "password",
                    "inviteCode": invite,
                })
                .to_string(),
            )
            .dispatch()
            .await;
        assert_eq!(response.status(), Status::Ok, "createAccount");
        let body: Value = response.into_json().await.unwrap();
        Harness {
            _dir: dir,
            client,
            did: body["did"].as_str().unwrap().to_string(),
            handle,
            token: body["accessJwt"].as_str().unwrap().to_string(),
        }
    }

    async fn curr(&self) -> i64 {
        self.client
            .rocket()
            .state::<SharedSequencer>()
            .unwrap()
            .sequencer
            .read()
            .await
            .curr()
            .await
            .unwrap()
            .unwrap_or(0)
    }

    async fn events_since(&self, cursor: i64) -> Vec<SeqEvt> {
        self.events_since_limit(cursor, None).await
    }

    async fn sequencer(&self) -> Sequencer {
        self.client
            .rocket()
            .state::<SharedSequencer>()
            .unwrap()
            .sequencer
            .read()
            .await
            .clone()
    }

    async fn events_since_limit(&self, cursor: i64, limit: Option<i64>) -> Vec<SeqEvt> {
        self.client
            .rocket()
            .state::<SharedSequencer>()
            .unwrap()
            .sequencer
            .read()
            .await
            .request_seq_range(RequestSeqRangeOpts {
                earliest_seq: Some(cursor),
                latest_seq: None,
                earliest_time: None,
                limit,
            })
            .await
            .unwrap()
    }

    async fn get(&self, path: &str) -> Value {
        let response = self
            .client
            .get(path)
            .header(Header::new(
                "Authorization",
                format!("Bearer {}", self.token),
            ))
            .dispatch()
            .await;
        assert_eq!(response.status(), Status::Ok, "GET {path}");
        response.into_json().await.unwrap()
    }

    async fn post(&self, path: &str, body: Value) -> (Status, Option<Value>) {
        let response = self
            .client
            .post(path)
            .header(ContentType::JSON)
            .header(Header::new(
                "Authorization",
                format!("Bearer {}", self.token),
            ))
            .body(body.to_string())
            .dispatch()
            .await;
        let status = response.status();
        (status, response.into_json().await)
    }

    fn record(&self, text: &str) -> Value {
        json!({ "$type": COLLECTION, "text": text, "createdAt": FIXED_TIME })
    }

    async fn write_record(&self, endpoint: &str, rkey: &str, text: &str) -> Cid {
        let (status, body) = self
            .post(
                &format!("/xrpc/com.atproto.repo.{endpoint}"),
                json!({
                    "repo": self.did,
                    "collection": COLLECTION,
                    "rkey": rkey,
                    "validate": false,
                    "record": self.record(text),
                }),
            )
            .await;
        assert_eq!(status, Status::Ok, "{endpoint} {rkey}");
        Cid::from_str(body.unwrap()["cid"].as_str().unwrap()).unwrap()
    }

    async fn create_record(&self, rkey: &str, text: &str) -> Cid {
        self.write_record("createRecord", rkey, text).await
    }

    async fn put_record(&self, rkey: &str, text: &str) -> Cid {
        self.write_record("putRecord", rkey, text).await
    }

    async fn delete_record(&self, rkey: &str) {
        let (status, _) = self
            .post(
                "/xrpc/com.atproto.repo.deleteRecord",
                json!({ "repo": self.did, "collection": COLLECTION, "rkey": rkey }),
            )
            .await;
        assert_eq!(status, Status::Ok, "deleteRecord {rkey}");
    }
}

fn commit(evt: &SeqEvt) -> &CommitEvt {
    match evt {
        SeqEvt::TypedCommitEvt(evt) => &evt.evt,
        other => panic!("expected a commit event, got {other:?}"),
    }
}

fn identity(evt: &SeqEvt) -> &IdentityEvt {
    match evt {
        SeqEvt::TypedIdentityEvt(evt) => &evt.evt,
        other => panic!("expected an identity event, got {other:?}"),
    }
}

fn sync(evt: &SeqEvt) -> &SyncEvt {
    match evt {
        SeqEvt::TypedSyncEvt(evt) => &evt.evt,
        other => panic!("expected a sync event, got {other:?}"),
    }
}

fn types_of(evts: &[SeqEvt]) -> Vec<&str> {
    evts.iter()
        .map(|evt| match evt {
            SeqEvt::TypedCommitEvt(_) => "commit",
            SeqEvt::TypedIdentityEvt(_) => "identity",
            SeqEvt::TypedAccountEvt(_) => "account",
            SeqEvt::TypedSyncEvt(_) => "sync",
        })
        .collect()
}

/// Splits a firehose frame into its two concatenated CBOR values: the header,
/// read as a generic map, and the still-encoded DAG-CBOR body.
fn split_frame(bytes: &[u8]) -> (CborValue, Vec<u8>) {
    let mut values = serde_cbor::Deserializer::from_slice(bytes).into_iter::<CborValue>();
    let header = values.next().expect("frame header").expect("frame header");
    let offset = values.byte_offset();
    (header, bytes[offset..].to_vec())
}

fn header_field(header: &CborValue, key: &str) -> Option<CborValue> {
    let CborValue::Map(map) = header else {
        panic!("frame header is not a cbor map");
    };
    map.get(&CborValue::Text(key.to_owned())).cloned()
}

fn frame_type(header: &CborValue) -> String {
    match header_field(header, "t") {
        Some(CborValue::Text(t)) => t,
        other => panic!("frame header has no string `t`: {other:?}"),
    }
}

/// Mirrors the dispatch `subscribe_repos` performs on each sequenced event:
/// build the lexicon body and tag the frame `#<event type>`.
fn encode_frame(evt: &SeqEvt) -> Vec<u8> {
    fn opts(r#type: &str) -> Option<MessageFrameOpts> {
        Some(MessageFrameOpts {
            r#type: Some(format!("#{type}")),
        })
    }
    match evt {
        SeqEvt::TypedCommitEvt(evt) => {
            let e = &evt.evt;
            MessageFrame::new(
                SubscribeReposCommit {
                    seq: evt.seq,
                    time: from_str_to_utc(&evt.time).unwrap(),
                    rebase: e.rebase,
                    too_big: e.too_big,
                    repo: e.repo.clone(),
                    commit: e.commit,
                    prev: e.prev,
                    rev: e.rev.clone(),
                    since: e.since.clone(),
                    blocks: e.blocks.clone(),
                    ops: e
                        .ops
                        .iter()
                        .map(|op| SubscribeReposCommitOperation {
                            path: op.path.clone(),
                            cid: op.cid,
                            prev: op.prev,
                            action: op.action.to_string(),
                        })
                        .collect(),
                    blobs: e.blobs.iter().map(|blob| blob.to_string()).collect(),
                    prev_data: e.prev_data,
                },
                opts(&evt.r#type),
            )
            .to_bytes()
            .unwrap()
        }
        SeqEvt::TypedIdentityEvt(evt) => MessageFrame::new(
            SubscribeReposIdentity {
                seq: evt.seq,
                did: evt.evt.did.clone(),
                handle: evt.evt.handle.clone(),
                time: from_str_to_utc(&evt.time).unwrap(),
            },
            opts(&evt.r#type),
        )
        .to_bytes()
        .unwrap(),
        SeqEvt::TypedAccountEvt(evt) => MessageFrame::new(
            SubscribeReposAccount {
                seq: evt.seq,
                did: evt.evt.did.clone(),
                active: evt.evt.active,
                status: evt.evt.status.clone(),
                time: from_str_to_utc(&evt.time).unwrap(),
            },
            opts(&evt.r#type),
        )
        .to_bytes()
        .unwrap(),
        SeqEvt::TypedSyncEvt(evt) => MessageFrame::new(
            SubscribeReposSync {
                seq: evt.seq,
                did: evt.evt.did.clone(),
                blocks: evt.evt.blocks.clone(),
                rev: evt.evt.rev.clone(),
                time: from_str_to_utc(&evt.time).unwrap(),
            },
            opts(&evt.r#type),
        )
        .to_bytes()
        .unwrap(),
    }
}

/// Reads the `data` field — the MST root — out of the commit block a commit
/// event's CAR is rooted at.
async fn data_root_of(evt: &CommitEvt) -> Cid {
    let car = read_car_with_root(evt.blocks.clone()).await.unwrap();
    assert_eq!(car.root, evt.commit, "CAR root is the commit CID");
    let block = car.blocks.get(car.root).expect("commit block in CAR");
    serde_ipld_dagcbor::from_slice::<Commit>(block)
        .unwrap()
        .data
}

// ------------------------------------------------------- websocket upgrade

#[tokio::test]
async fn subscribe_repos_rejects_a_non_websocket_request() {
    let (_dir, client) = common::get_client().await;
    let response = client
        .get("/xrpc/com.atproto.sync.subscribeRepos")
        .dispatch()
        .await;
    let status = response.status();
    let body: Value = response.into_json().await.unwrap();
    assert_eq!(status, Status::BadRequest);
    assert_eq!(body["error"], "InvalidRequest");
}

#[tokio::test]
async fn subscribe_repos_accepts_a_websocket_handshake() {
    let (_dir, client) = common::get_client().await;
    let response = client
        .get("/xrpc/com.atproto.sync.subscribeRepos?cursor=0")
        .header(Header::new("Connection", "Upgrade"))
        .header(Header::new("Upgrade", "websocket"))
        .header(Header::new("Sec-WebSocket-Version", "13"))
        .header(Header::new("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ=="))
        .dispatch()
        .await;
    // The RFC 6455 accept value for the fixed key above.
    assert_eq!(
        response.headers().get_one("Sec-WebSocket-Accept"),
        Some("s3pPLMBiTxaQ9kYGzzhZRbK+xOo=")
    );
}

// -------------------------------------------------------- event sequencing

#[tokio::test]
async fn account_creation_sequences_identity_account_commit_and_sync() {
    let harness = Harness::new().await;
    let events = harness.events_since(0).await;

    assert_eq!(
        types_of(&events),
        vec!["identity", "account", "commit", "sync"]
    );
    assert_eq!(identity(&events[0]).did, harness.did);
    assert_eq!(identity(&events[0]).handle, Some(harness.handle.clone()));
    assert_eq!(sync(&events[3]).did, harness.did);

    let genesis = commit(&events[2]);
    assert_eq!(genesis.repo, harness.did);
    assert_eq!(genesis.ops.len(), 0);
    // The genesis commit has no predecessor, so there is no prior MST root for
    // a relay to invert against.
    assert_eq!(genesis.prev_data, None);
}

#[tokio::test]
async fn create_record_is_sequenced_as_a_commit_event() {
    let harness = Harness::new().await;
    let cursor = harness.curr().await;
    let cid = harness.create_record("create-seq", "sequence me").await;

    let events = harness.events_since(cursor).await;
    assert_eq!(types_of(&events), vec!["commit"]);
    let evt = commit(&events[0]);
    assert_eq!(evt.repo, harness.did);
    assert_eq!(evt.ops.len(), 1);
    assert_eq!(evt.ops[0].action, CommitEvtOpAction::Create);
    assert_eq!(evt.ops[0].path, format!("{COLLECTION}/create-seq"));
    assert_eq!(evt.ops[0].cid, Some(cid));
}

#[tokio::test]
async fn delete_record_is_sequenced_as_a_commit_event() {
    let harness = Harness::new().await;
    harness.create_record("delete-seq", "to delete").await;
    let cursor = harness.curr().await;
    harness.delete_record("delete-seq").await;

    let events = harness.events_since(cursor).await;
    assert_eq!(types_of(&events), vec!["commit"]);
    let evt = commit(&events[0]);
    assert_eq!(evt.ops.len(), 1);
    assert_eq!(evt.ops[0].action, CommitEvtOpAction::Delete);
    assert_eq!(evt.ops[0].path, format!("{COLLECTION}/delete-seq"));
    assert_eq!(evt.ops[0].cid, None);
}

// ----------------------------------------------------- cursors & retrieval

#[tokio::test]
async fn events_are_backfilled_from_a_cursor() {
    let harness = Harness::new().await;
    let cursor = harness.curr().await;
    for i in 0..3 {
        harness.create_record(&format!("backfill-{i}"), "b").await;
    }

    let events = harness.events_since(cursor).await;
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].seq(), cursor + 1);
    assert_eq!(events[2].seq(), cursor + 3);
    // A cursor at the head yields nothing until the next write.
    assert_eq!(harness.events_since(cursor + 3).await.len(), 0);
}

#[tokio::test]
async fn event_retrieval_respects_the_limit() {
    let harness = Harness::new().await;
    let cursor = harness.curr().await;
    for i in 0..10 {
        harness.create_record(&format!("limit-{i}"), "l").await;
    }

    assert_eq!(harness.events_since(cursor).await.len(), 10);
    let limited = harness.events_since_limit(cursor, Some(5)).await;
    assert_eq!(limited.len(), 5);
    assert_eq!(limited[4].seq(), cursor + 5);
}

#[tokio::test]
async fn cursor_primitives_report_the_next_and_current_sequence() {
    let harness = Harness::new().await;
    let head = harness.curr().await;
    harness.create_record("cursor-primitives", "c").await;

    let sequencer = harness.sequencer().await;
    assert_eq!(sequencer.curr().await.unwrap(), Some(head + 1));
    assert_eq!(
        sequencer.next_seq(head).await.unwrap().unwrap().seq,
        Some(head + 1)
    );
    assert!(sequencer.next_seq(head + 1).await.unwrap().is_none());
    // Nothing was sequenced after the far future, which is the condition
    // `subscribe_repos` uses to decide a cursor has outrun the backfill window.
    assert!(sequencer
        .earliest_after_time("2100-01-01T00:00:00.000Z".to_owned())
        .await
        .unwrap()
        .is_none());
}

/// The outbox is what `subscribe_repos` puts between the sequencer and the
/// socket, so a cursor's backfill is really an `Outbox::get_backfill` drain.
#[tokio::test]
async fn the_outbox_backfills_every_event_after_a_zero_cursor() {
    let harness = Harness::new().await;
    harness.create_record("outbox-a", "a").await;
    harness.create_record("outbox-b", "b").await;

    let mut outbox = Outbox::new(
        harness.sequencer().await,
        Some(OutboxOpts {
            max_buffer_size: 500,
        }),
    );
    let backfill: Vec<SeqEvt> = outbox
        .get_backfill(0)
        .await
        .map(|evt| evt.unwrap())
        .collect()
        .await;

    assert_eq!(
        types_of(&backfill),
        vec!["identity", "account", "commit", "sync", "commit", "commit"]
    );
    assert_eq!(backfill[0].seq(), 1);
    assert_eq!(backfill[5].seq(), 6);
    assert_eq!(outbox.last_seen, 6);
}

#[tokio::test]
async fn the_outbox_backfills_only_what_follows_a_mid_stream_cursor() {
    let harness = Harness::new().await;
    let cursor = harness.curr().await;
    harness.create_record("outbox-mid", "m").await;

    let mut outbox = Outbox::new(harness.sequencer().await, None);
    let backfill: Vec<SeqEvt> = outbox
        .get_backfill(cursor)
        .await
        .map(|evt| evt.unwrap())
        .collect()
        .await;

    assert_eq!(types_of(&backfill), vec!["commit"]);
    assert_eq!(backfill[0].seq(), cursor + 1);
    // A cursor at the head backfills nothing.
    let mut caught_up = Outbox::new(harness.sequencer().await, None);
    let empty: Vec<SeqEvt> = caught_up
        .get_backfill(cursor + 1)
        .await
        .map(|evt| evt.unwrap())
        .collect()
        .await;
    assert_eq!(empty.len(), 0);
}

// ------------------------------------------------------------ event blocks

#[tokio::test]
async fn commit_events_carry_a_car_of_the_written_blocks() {
    let harness = Harness::new().await;
    let cursor = harness.curr().await;
    let cid = harness.create_record("blocks-test", "block content").await;

    let events = harness.events_since(cursor).await;
    let evt = commit(&events[0]);
    let car = read_car_with_root(evt.blocks.clone()).await.unwrap();
    assert_eq!(car.root, evt.commit);
    assert!(car.blocks.has(cid), "record block is in the commit CAR");
    assert!(
        car.blocks.has(car.root),
        "commit block is in the commit CAR"
    );
}

#[tokio::test]
async fn commit_event_blocks_are_never_empty() {
    let harness = Harness::new().await;
    let cursor = harness.curr().await;
    harness.create_record("non-empty", "must have blocks").await;

    let events = harness.events_since(cursor).await;
    assert_eq!(events.len(), 1);
    let evt = commit(&events[0]);
    assert!(!evt.blocks.is_empty());
    let car = read_car_with_root(evt.blocks.clone()).await.unwrap();
    assert_eq!(car.blocks.size(), 3, "commit, MST root and record blocks");
}

// ---------------------------------------------------------- frame encoding

#[tokio::test]
async fn commit_events_encode_as_a_commit_frame() {
    let harness = Harness::new().await;
    let cursor = harness.curr().await;
    harness.create_record("frame-commit", "frame me").await;

    let events = harness.events_since(cursor).await;
    assert_eq!(events.len(), 1);
    let (header, body) = split_frame(&encode_frame(&events[0]));
    assert_eq!(header_field(&header, "op"), Some(CborValue::Integer(1)));
    assert_eq!(frame_type(&header), "#commit");

    let decoded: SubscribeReposCommit = serde_ipld_dagcbor::from_slice(&body).unwrap();
    assert_eq!(decoded.repo, harness.did);
    assert_eq!(decoded.seq, events[0].seq());
    assert!(!decoded.too_big);
    assert!(decoded.prev_data.is_some());
    assert_eq!(decoded.ops.len(), 1);
    assert_eq!(decoded.ops[0].action, "create");
    assert_eq!(decoded.ops[0].prev, None);
}

#[tokio::test]
async fn identity_events_encode_as_an_identity_frame() {
    let harness = Harness::new().await;
    let events = harness.events_since(0).await;
    let (header, body) = split_frame(&encode_frame(&events[0]));

    assert_eq!(header_field(&header, "op"), Some(CborValue::Integer(1)));
    assert_eq!(frame_type(&header), "#identity");
    let decoded: SubscribeReposIdentity = serde_ipld_dagcbor::from_slice(&body).unwrap();
    assert_eq!(decoded.did, harness.did);
    assert_eq!(decoded.handle, Some(harness.handle.clone()));
}

#[tokio::test]
async fn account_and_sync_events_encode_to_their_own_frame_types() {
    let harness = Harness::new().await;
    let events = harness.events_since(0).await;
    assert_eq!(
        types_of(&events),
        vec!["identity", "account", "commit", "sync"]
    );

    let (account_header, account_body) = split_frame(&encode_frame(&events[1]));
    assert_eq!(frame_type(&account_header), "#account");
    let account: SubscribeReposAccount = serde_ipld_dagcbor::from_slice(&account_body).unwrap();
    assert_eq!(account.did, harness.did);
    assert!(account.active);
    assert!(account.status.is_none());

    let (sync_header, sync_body) = split_frame(&encode_frame(&events[3]));
    assert_eq!(frame_type(&sync_header), "#sync");
    let sync_evt: SubscribeReposSync = serde_ipld_dagcbor::from_slice(&sync_body).unwrap();
    assert_eq!(sync_evt.did, harness.did);
    assert!(!sync_evt.blocks.is_empty());
}

#[tokio::test]
async fn each_event_type_dispatches_to_its_own_encoder() {
    let harness = Harness::new().await;
    let events = harness.events_since(0).await;
    let tags: Vec<String> = events
        .iter()
        .map(|evt| frame_type(&split_frame(&encode_frame(evt)).0))
        .collect();
    assert_eq!(tags, vec!["#identity", "#account", "#commit", "#sync"]);
}

#[tokio::test]
async fn an_outdated_cursor_encodes_as_an_info_frame() {
    let frame = MessageFrame::new(
        InfoFrameBody {
            name: "OutdatedCursor".to_owned(),
            message: Some("Requested cursor exceeded limit. Possibly missing events".to_owned()),
        },
        Some(MessageFrameOpts {
            r#type: Some("#info".to_owned()),
        }),
    );
    let (header, body) = split_frame(&frame.to_bytes().unwrap());
    assert_eq!(header_field(&header, "op"), Some(CborValue::Integer(1)));
    assert_eq!(frame_type(&header), "#info");
    let decoded: InfoFrameBody = serde_ipld_dagcbor::from_slice(&body).unwrap();
    assert_eq!(decoded.name, "OutdatedCursor");
}

// --------------------------------------------------------- identity events

#[tokio::test]
async fn an_identity_event_may_omit_the_handle() {
    let harness = Harness::new().await;
    let cursor = harness.curr().await;
    harness
        .client
        .rocket()
        .state::<SharedSequencer>()
        .unwrap()
        .sequencer
        .write()
        .await
        .sequence_identity_evt(harness.did.clone(), None)
        .await
        .unwrap();

    let events = harness.events_since(cursor).await;
    assert_eq!(types_of(&events), vec!["identity"]);
    assert_eq!(identity(&events[0]).did, harness.did);
    assert_eq!(identity(&events[0]).handle, None);

    let (header, body) = split_frame(&encode_frame(&events[0]));
    assert_eq!(frame_type(&header), "#identity");
    let decoded: SubscribeReposIdentity = serde_ipld_dagcbor::from_slice(&body).unwrap();
    assert_eq!(decoded.handle, None);
}

// ------------------------------------------------------- sync 1.1 #commit

#[tokio::test]
async fn every_commit_after_the_genesis_commit_carries_prev_data() {
    let harness = Harness::new().await;
    let cursor = harness.curr().await;
    harness.create_record("prevdata-1", "first").await;
    harness.create_record("prevdata-2", "second").await;

    let events = harness.events_since(cursor).await;
    assert_eq!(events.len(), 2);
    let first = commit(&events[0]);
    let second = commit(&events[1]);
    assert!(first.prev_data.is_some());
    assert!(second.prev_data.is_some());
    // Each write moves the MST root, so consecutive commits cannot share one.
    assert_ne!(first.prev_data, second.prev_data);
}

#[tokio::test]
async fn too_big_is_always_false() {
    let harness = Harness::new().await;
    harness.create_record("toobig", "small").await;

    for event in &harness.events_since(0).await {
        if let SeqEvt::TypedCommitEvt(evt) = event {
            assert!(!evt.evt.too_big);
            assert!(!evt.evt.rebase);
            // `prev` is deprecated in sync 1.1 and must not be emitted.
            assert_eq!(evt.evt.prev, None);
        }
    }
}

#[tokio::test]
async fn op_prev_is_set_on_update_and_delete_and_omitted_on_create() {
    let harness = Harness::new().await;

    let created = harness.create_record("prev-op", "v1").await;
    let update_cursor = harness.curr().await;
    let updated = harness.put_record("prev-op", "v2").await;
    let delete_cursor = harness.curr().await;
    harness.delete_record("prev-op").await;

    let update_events = harness.events_since(update_cursor).await;
    let update_op = &commit(&update_events[0]).ops[0];
    assert_eq!(update_op.action, CommitEvtOpAction::Update);
    assert_eq!(update_op.cid, Some(updated));
    assert_eq!(update_op.prev, Some(created));

    let delete_events = harness.events_since(delete_cursor).await;
    let delete_op = &commit(&delete_events[0]).ops[0];
    assert_eq!(delete_op.action, CommitEvtOpAction::Delete);
    assert_eq!(delete_op.cid, None);
    assert_eq!(delete_op.prev, Some(updated));

    let create_cursor = harness.curr().await;
    harness.create_record("no-prev-op", "fresh").await;
    let create_events = harness.events_since(create_cursor).await;
    let create_op = &commit(&create_events[0]).ops[0];
    assert_eq!(create_op.action, CommitEvtOpAction::Create);
    assert_eq!(create_op.prev, None);
}

#[tokio::test]
async fn prev_data_on_a_commit_equals_the_previous_commits_mst_root() {
    let harness = Harness::new().await;
    let cursor = harness.curr().await;
    harness.create_record("link-a", "a").await;
    harness.create_record("link-b", "b").await;

    let events = harness.events_since(cursor).await;
    assert_eq!(events.len(), 2);
    let first = commit(&events[0]);
    let second = commit(&events[1]);
    assert_eq!(second.prev_data, Some(data_root_of(first).await));

    // and the first commit's prevData is the genesis commit's MST root
    let from_genesis = harness.events_since(0).await;
    let genesis = commit(&from_genesis[2]);
    assert_eq!(first.prev_data, Some(data_root_of(genesis).await));
}

#[tokio::test]
async fn apply_writes_rejects_more_than_two_hundred_operations() {
    let harness = Harness::new().await;
    let write = |i: usize| {
        json!({
            "$type": "com.atproto.repo.applyWrites#create",
            "collection": COLLECTION,
            "rkey": format!("bulk-{i}"),
            "value": harness.record("bulk"),
        })
    };

    let cursor = harness.curr().await;
    let too_many: Vec<Value> = (0..201).map(write).collect();
    let (status, _) = harness
        .post(
            "/xrpc/com.atproto.repo.applyWrites",
            json!({ "repo": harness.did, "validate": false, "writes": too_many }),
        )
        .await;
    // @NOTE the reference PDS answers 400 InvalidRequest here; rsky-pds funnels
    // the bail through ApiError::RuntimeError.
    assert_eq!(status, Status::InternalServerError);
    assert_eq!(harness.events_since(cursor).await.len(), 0);

    let at_the_cap: Vec<Value> = (0..200).map(write).collect();
    let (status, _) = harness
        .post(
            "/xrpc/com.atproto.repo.applyWrites",
            json!({ "repo": harness.did, "validate": false, "writes": at_the_cap }),
        )
        .await;
    assert_eq!(status, Status::Ok);
    let events = harness.events_since(cursor).await;
    assert_eq!(types_of(&events), vec!["commit"]);
    assert_eq!(commit(&events[0]).ops.len(), 200);
}

// -------------------------------------------- sync 1.1 #account and #sync

#[tokio::test]
async fn activate_account_emits_account_identity_and_sync_events() {
    let harness = Harness::new().await;
    harness.create_record("ensure-root", "x").await;

    let credentials = harness
        .get("/xrpc/com.atproto.identity.getRecommendedDidCredentials")
        .await;
    let signing_key = credentials["verificationMethods"]["atproto"]
        .as_str()
        .unwrap()
        .to_string();
    common::set_published_signing_key(Some(signing_key));

    let cursor = harness.curr().await;
    let (status, _) = harness
        .post("/xrpc/com.atproto.server.activateAccount", json!({}))
        .await;
    common::set_published_signing_key(None);
    assert_eq!(status, Status::Ok, "activateAccount");

    let events = harness.events_since(cursor).await;
    assert_eq!(types_of(&events), vec!["account", "identity", "sync"]);

    let SeqEvt::TypedAccountEvt(account) = &events[0] else {
        panic!("expected an account event");
    };
    assert_eq!(account.evt.did, harness.did);
    assert!(account.evt.active);
    assert_eq!(account.evt.status, None);
    assert_eq!(identity(&events[1]).did, harness.did);

    let sync_evt = sync(&events[2]);
    assert_eq!(sync_evt.did, harness.did);
    let car = read_car_with_root(sync_evt.blocks.clone()).await.unwrap();
    // A #sync event carries exactly the current signed commit block.
    assert_eq!(car.blocks.size(), 1);
    let commit_block = car.blocks.get(car.root).expect("commit block in CAR");
    let decoded = serde_ipld_dagcbor::from_slice::<Commit>(commit_block).unwrap();
    assert_eq!(decoded.did, harness.did);
    assert_eq!(decoded.rev, sync_evt.rev);

    let (header, body) = split_frame(&encode_frame(&events[2]));
    assert_eq!(frame_type(&header), "#sync");
    let framed: SubscribeReposSync = serde_ipld_dagcbor::from_slice(&body).unwrap();
    assert_eq!(framed.rev, sync_evt.rev);
}

/// DIVERGENCE (sync 1.1): `com.atproto.server.deactivateAccount` sequences no
/// `#account` event, so a relay never learns the repo went inactive. The
/// reference PDS emits `#account(active=false, status=deactivated)`.
/// See `rsky-pds/src/apis/com/atproto/server/deactivate_account.rs`.
#[tokio::test]
#[ignore = "known divergence: deactivateAccount emits no #account event"]
async fn deactivate_account_emits_an_inactive_account_event() {
    let harness = Harness::new().await;
    let cursor = harness.curr().await;
    let (status, _) = harness
        .post("/xrpc/com.atproto.server.deactivateAccount", json!({}))
        .await;
    assert_eq!(status, Status::Ok, "deactivateAccount");

    let events = harness.events_since(cursor).await;
    assert_eq!(types_of(&events), vec!["account"]);
    let SeqEvt::TypedAccountEvt(account) = &events[0] else {
        panic!("expected an account event");
    };
    assert!(!account.evt.active);
    assert_eq!(account.evt.status, Some(AccountStatus::Deactivated));
}
