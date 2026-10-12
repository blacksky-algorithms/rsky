//! The feed routes through the real proxy path: the appview's page arrives
//! stamped with the repo rev of the account's older write, so the PDS inserts
//! the newer pending records and must hand every other key back as it arrived.

mod common;

use rocket::http::{ContentType, Header, Status};
use rocket::local::asynchronous::Client;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Once;

const ACCOUNT_DID: &str = "did:plc:khvyd3oiw46vif5gm7hijslk";
const OTHER_DID: &str = "did:plc:7s2nftzuiyp3px3s2zqwy2zt";
const CID: &str = "bafkreibjfgx2gprinfvicegelk5kosd6y2frmqpqzwqkg7usac74l3t2v4";
/// A rev the PDS has not seen is read as clock skew and ignored, so each
/// request names the rev the mock should stamp its answer with.
const REV_PARAM: &str = "rev";
const INDEXED_AT: &str = "2024-01-01T00:00:00.000Z";
const THREAD_VIEW_POST: &str = "app.bsky.feed.defs#threadViewPost";

static APPVIEW: Once = Once::new();

fn other_uri(rkey: &str) -> String {
    format!("at://{OTHER_DID}/app.bsky.feed.post/{rkey}")
}

fn decode(value: &str) -> String {
    value
        .replace("%3A", ":")
        .replace("%3a", ":")
        .replace("%2F", "/")
        .replace("%2f", "/")
}

fn encode(value: &str) -> String {
    value.replace(':', "%3A").replace('/', "%2F")
}

fn param(query: &str, name: &str) -> String {
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
        .map(decode)
        .unwrap_or_default()
}

/// A post view carrying the keys the typed lexicon structs do not describe.
fn post_view(did: &str, uri: &str) -> Value {
    json!({
        "uri": uri,
        "cid": CID,
        "author": { "did": did, "handle": "someone.test", "verification": { "verifiedStatus": "valid" } },
        "record": { "$type": "app.bsky.feed.post", "text": "hi", "createdAt": INDEXED_AT },
        "indexedAt": INDEXED_AT,
        "viewer": {
            "repost": format!("at://{did}/app.bsky.feed.repost/r"),
            "like": format!("at://{did}/app.bsky.feed.like/l"),
            "threadMuted": true
        },
        "quoteCount": 7,
        "bookmarkCount": 1
    })
}

fn feed_item(did: &str, uri: &str) -> Value {
    json!({ "post": post_view(did, uri), "feedContext": "ctx" })
}

fn reposted_item() -> Value {
    json!({
        "post": post_view(OTHER_DID, &other_uri("reposted")),
        "reason": {
            "$type": "app.bsky.feed.defs#reasonRepost",
            "by": { "did": ACCOUNT_DID, "handle": "foo.rsky.com", "viewer": { "mutedOnlyReposts": true } },
            "uri": format!("at://{ACCOUNT_DID}/app.bsky.feed.repost/x"),
            "cid": CID,
            "indexedAt": INDEXED_AT
        }
    })
}

fn thread_node(uri: &str) -> Value {
    json!({ "$type": THREAD_VIEW_POST, "post": post_view(OTHER_DID, uri), "replies": [] })
}

fn appview_response(path: &str, query: &str) -> (u16, Value) {
    let uri = param(query, "uri");
    let actor = param(query, "actor");
    match path {
        "/xrpc/app.bsky.feed.getTimeline" => (
            200,
            json!({ "feed": [reposted_item(), feed_item(OTHER_DID, &other_uri("older"))] }),
        ),
        "/xrpc/app.bsky.feed.getAuthorFeed" => (
            200,
            json!({ "feed": [feed_item(&actor, &format!("at://{actor}/app.bsky.feed.post/published"))] }),
        ),
        "/xrpc/app.bsky.feed.getActorLikes" => (
            200,
            json!({ "feed": [feed_item(OTHER_DID, &other_uri("liked"))] }),
        ),
        "/xrpc/app.bsky.feed.getPostThread" if uri.starts_with(&format!("at://{ACCOUNT_DID}/")) => {
            (
                400,
                json!({ "error": "NotFound", "message": "Post not found" }),
            )
        }
        "/xrpc/app.bsky.feed.getPostThread" => (200, json!({ "thread": thread_node(&uri) })),
        _ => (404, json!({ "error": "NotFound", "message": path })),
    }
}

/// Serves the pages above for any caller, each stamped with the repo rev the
/// request names, which makes the PDS apply the records written after it.
fn start_mock_appview() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock appview");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut raw = Vec::new();
            let mut buf = [0u8; 4096];
            while !raw.windows(4).any(|window| window == b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                raw.extend_from_slice(&buf[..n]);
            }
            let head = String::from_utf8_lossy(&raw).to_string();
            let target = head.split_whitespace().nth(1).unwrap_or("/");
            let (path, query) = target.split_once('?').unwrap_or((target, ""));
            let (status, body) = appview_response(path, query);
            let body = body.to_string();
            let rev = param(query, REV_PARAM);
            let reason = if status == 200 { "OK" } else { "Bad Request" };
            let _ = write!(
                stream,
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\n\
                 atproto-repo-rev: {rev}\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    port
}

/// Development mode lets the proxy reach the plain-http mock on loopback.
fn configure_appview() {
    APPVIEW.call_once(|| {
        let port = start_mock_appview();
        std::env::set_var("PDS_DEV_MODE", "true");
        std::env::set_var("PDS_BSKY_APP_VIEW_URL", format!("http://127.0.0.1:{port}"));
        std::env::set_var("PDS_BSKY_APP_VIEW_DID", "did:web:appview.invalid");
    });
}

/// A live account with one published write, and the repo rev of that write.
async fn signed_in_client() -> (tempfile::TempDir, Client, String, String) {
    configure_appview();
    let (dir, client) = common::get_client().await;
    let (identifier, password) = common::create_account(&client).await;
    // Accounts created with a supplied did start deactivated; writes need it live.
    client
        .rocket()
        .state::<rsky_pds::account_manager::AccountManager>()
        .unwrap()
        .activate_account(ACCOUNT_DID)
        .await
        .unwrap();
    let response = client
        .post("/xrpc/com.atproto.server.createSession")
        .header(ContentType::JSON)
        .body(json!({ "identifier": identifier, "password": password }).to_string())
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: Value = response.into_json().await.unwrap();
    let token = body["accessJwt"].as_str().unwrap().to_owned();
    create_post(&client, &token, plain_post("published earlier")).await;
    let response = client
        .get(format!(
            "/xrpc/com.atproto.sync.getLatestCommit?did={ACCOUNT_DID}"
        ))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: Value = response.into_json().await.unwrap();
    let rev = body["rev"].as_str().unwrap().to_owned();
    (dir, client, token, rev)
}

fn plain_post(text: &str) -> Value {
    json!({ "$type": "app.bsky.feed.post", "text": text, "createdAt": "2026-01-01T00:00:00.000Z" })
}

fn reply_post(text: &str, root: &str, parent: &str) -> Value {
    let mut record = plain_post(text);
    record["reply"] = json!({
        "root": { "uri": root, "cid": CID },
        "parent": { "uri": parent, "cid": CID }
    });
    record
}

async fn create_post(client: &Client, token: &str, record: Value) -> String {
    let response = client
        .post("/xrpc/com.atproto.repo.createRecord")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .body(
            json!({ "repo": ACCOUNT_DID, "collection": "app.bsky.feed.post", "record": record })
                .to_string(),
        )
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: Value = response.into_json().await.unwrap();
    body["uri"].as_str().unwrap().to_owned()
}

/// The page the PDS answers with and whether it reported the pending write.
async fn fetch(client: &Client, token: &str, path: &str) -> (bool, Value) {
    let response = client
        .get(path)
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let lagged = response.headers().get_one("Atproto-Upstream-Lag").is_some();
    (lagged, response.into_json().await.unwrap())
}

#[tokio::test]
async fn timeline_keeps_the_appview_items_intact_while_inserting_pending_posts() {
    let (_dir, client, token, rev) = signed_in_client().await;
    let pending = create_post(&client, &token, plain_post("just written")).await;
    let path = format!("/xrpc/app.bsky.feed.getTimeline?limit=10&{REV_PARAM}={rev}");
    let (lagged, body) = fetch(&client, &token, &path).await;
    assert!(lagged);
    let feed = body["feed"].as_array().unwrap();
    assert_eq!(feed.len(), 3);
    assert_eq!(feed[0]["post"]["uri"], json!(pending));
    assert_eq!(feed[1], reposted_item());
    assert_eq!(feed[2], feed_item(OTHER_DID, &other_uri("older")));
}

#[tokio::test]
async fn author_feed_inserts_pending_posts_only_into_the_requesters_feed() {
    let (_dir, client, token, rev) = signed_in_client().await;
    let pending = create_post(&client, &token, plain_post("just written")).await;
    let own =
        format!("/xrpc/app.bsky.feed.getAuthorFeed?actor={ACCOUNT_DID}&limit=10&{REV_PARAM}={rev}");
    let (lagged, body) = fetch(&client, &token, &own).await;
    assert!(lagged);
    let feed = body["feed"].as_array().unwrap();
    assert_eq!(feed.len(), 2);
    assert_eq!(feed[0]["post"]["uri"], json!(pending));
    assert_eq!(
        feed[1],
        feed_item(
            ACCOUNT_DID,
            &format!("at://{ACCOUNT_DID}/app.bsky.feed.post/published")
        )
    );
    let foreign =
        format!("/xrpc/app.bsky.feed.getAuthorFeed?actor={OTHER_DID}&limit=10&{REV_PARAM}={rev}");
    let (lagged, body) = fetch(&client, &token, &foreign).await;
    assert!(lagged);
    assert_eq!(
        body,
        json!({ "feed": [feed_item(OTHER_DID, &other_uri("published"))] })
    );
}

#[tokio::test]
async fn actor_likes_pass_through_unchanged_with_the_lag_header() {
    let (_dir, client, token, rev) = signed_in_client().await;
    create_post(&client, &token, plain_post("just written")).await;
    let path =
        format!("/xrpc/app.bsky.feed.getActorLikes?actor={ACCOUNT_DID}&limit=10&{REV_PARAM}={rev}");
    let (lagged, body) = fetch(&client, &token, &path).await;
    assert!(lagged);
    assert_eq!(
        body,
        json!({ "feed": [feed_item(OTHER_DID, &other_uri("liked"))] })
    );
}

#[tokio::test]
async fn post_thread_grafts_pending_replies_onto_the_appview_thread() {
    let anchor = other_uri("anchor");
    let (_dir, client, token, rev) = signed_in_client().await;
    let reply = create_post(&client, &token, reply_post("me too", &anchor, &anchor)).await;
    let path = format!(
        "/xrpc/app.bsky.feed.getPostThread?uri={}&{REV_PARAM}={rev}",
        encode(&anchor)
    );
    let (lagged, body) = fetch(&client, &token, &path).await;
    assert!(lagged);
    assert_eq!(body["thread"]["post"], post_view(OTHER_DID, &anchor));
    let replies = body["thread"]["replies"].as_array().unwrap();
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["$type"], json!(THREAD_VIEW_POST));
    assert_eq!(replies[0]["post"]["uri"], json!(reply));
}

#[tokio::test]
async fn post_thread_builds_a_pending_post_the_appview_has_not_indexed() {
    let parent = other_uri("parent");
    let (_dir, client, token, rev) = signed_in_client().await;
    let reply = create_post(&client, &token, reply_post("new reply", &parent, &parent)).await;
    let path = format!(
        "/xrpc/app.bsky.feed.getPostThread?uri={}&{REV_PARAM}={rev}",
        encode(&reply)
    );
    let (lagged, body) = fetch(&client, &token, &path).await;
    assert!(lagged);
    assert_eq!(body["thread"]["$type"], json!(THREAD_VIEW_POST));
    assert_eq!(body["thread"]["post"]["uri"], json!(reply));
    assert_eq!(body["thread"]["parent"]["$type"], json!(THREAD_VIEW_POST));
    assert_eq!(body["thread"]["parent"]["post"]["uri"], json!(parent));
    assert_eq!(
        body["thread"]["parent"]["post"]["viewer"]["repost"],
        json!(format!("at://{OTHER_DID}/app.bsky.feed.repost/r"))
    );
}
