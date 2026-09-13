//! `PDS_READ_ONLY=true`: every user and service database stays byte for
//! byte what it was, mutating requests are refused before any handler
//! runs, and reads keep working over a reference-created data directory.

mod common;

use common::get_client_with_fixture_copy;
use rocket::http::{ContentType, Header, Status};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

/// Every user and service database under `data`, with the rsky control
/// journals under `data/rsky` left out.
fn database_digests(data: &Path) -> BTreeMap<String, String> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name == "rsky") {
                    continue;
                }
                walk(&path, root, out);
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if name.ends_with(".sqlite") || name.ends_with(".sqlite-wal") {
                let bytes = std::fs::read(&path).unwrap();
                // a read-only connection creates an empty write-ahead log
                // beside a checkpointed database; it holds no change
                if bytes.is_empty() {
                    continue;
                }
                let digest = Sha256::digest(bytes);
                let relative = path.strip_prefix(root).unwrap().display().to_string();
                out.insert(relative, format!("{digest:x}"));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(data, data, &mut out);
    out
}

#[tokio::test]
async fn read_only_mode_serves_reads_and_writes_nothing() {
    std::env::set_var("PDS_READ_ONLY", "true");
    let (fixture, dir, client) = get_client_with_fixture_copy().await;
    let data = dir.path().join("data");
    let before = database_digests(&data);
    assert!(before.len() >= 4, "{before:?}");
    let alice = fixture.did("alice");
    let alice_token = fixture.token("alice_access");

    for path in [
        format!("/xrpc/com.atproto.sync.getRepo?did={alice}"),
        format!("/xrpc/com.atproto.sync.getLatestCommit?did={alice}"),
        format!("/xrpc/com.atproto.repo.describeRepo?repo={alice}"),
        "/xrpc/com.atproto.server.describeServer".to_string(),
        "/xrpc/com.atproto.identity.resolveHandle?handle=alice.fixture.test".to_string(),
    ] {
        let response = client.get(&path).dispatch().await;
        assert_eq!(response.status(), Status::Ok, "{path}");
    }
    let session = client
        .get("/xrpc/com.atproto.server.getSession")
        .header(Header::new(
            "Authorization",
            format!("Bearer {alice_token}"),
        ))
        .dispatch()
        .await;
    assert_eq!(session.status(), Status::Ok);

    async fn refused(response: rocket::local::asynchronous::LocalResponse<'_>) {
        assert_eq!(response.status(), Status::ServiceUnavailable);
        let body: Value = response.into_json().await.unwrap();
        assert_eq!(body["error"], "ReadOnly", "{body}");
    }
    refused(
        client
            .post("/xrpc/com.atproto.server.createSession")
            .header(ContentType::JSON)
            .body(
                json!({
                    "identifier": "alice.fixture.test",
                    "password": fixture.account("alice")["password"]
                })
                .to_string(),
            )
            .dispatch()
            .await,
    )
    .await;
    refused(
        client
            .post("/xrpc/com.atproto.repo.createRecord")
            .header(ContentType::JSON)
            .header(Header::new(
                "Authorization",
                format!("Bearer {alice_token}"),
            ))
            .body(
                json!({
                    "repo": alice,
                    "collection": "app.bsky.feed.post",
                    "record": {"$type": "app.bsky.feed.post", "text": "never", "createdAt": "2026-09-12T00:00:00.000Z"}
                })
                .to_string(),
            )
            .dispatch()
            .await,
    )
    .await;
    refused(
        client
            .post("/xrpc/com.atproto.server.createInviteCode")
            .header(ContentType::JSON)
            .header(Header::new("Authorization", common::get_admin_token()))
            .body(json!({"useCount": 1}).to_string())
            .dispatch()
            .await,
    )
    .await;
    refused(client.put("/xrpc/anything").dispatch().await).await;
    refused(client.delete("/xrpc/anything").dispatch().await).await;
    refused(client.patch("/xrpc/anything").dispatch().await).await;

    let after = database_digests(&data);
    assert_eq!(before, after);
    // the control journals stay writable: the exports above recorded
    // their exposure there
    assert!(data.join("rsky").join("lifecycle.sqlite").exists());
}
