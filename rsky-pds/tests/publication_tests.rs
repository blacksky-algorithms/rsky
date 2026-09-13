//! Writes through the public routes publish exactly once, and a restart
//! finishes what a crash left behind.

mod common;

use common::{create_account, get_admin_token, get_client_in, sequencer_events};
use rocket::http::{ContentType, Header, Status};
use rocket::local::asynchronous::Client;
use rocket::serde::json::json;
use serde_json::Value;
use sha2::{Digest, Sha256};

const DID: &str = "did:plc:khvyd3oiw46vif5gm7hijslk";

async fn session_token(client: &Client, identifier: &str, password: &str) -> String {
    let response = client
        .post("/xrpc/com.atproto.server.createSession")
        .header(ContentType::JSON)
        .body(json!({"identifier": identifier, "password": password}).to_string())
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: Value = response.into_json().await.unwrap();
    format!("Bearer {}", body["accessJwt"].as_str().unwrap())
}

async fn post(client: &Client, path: &str, token: &str, body: Value) -> (Status, Value) {
    let response = client
        .post(path)
        .header(ContentType::JSON)
        .header(Header::new("Authorization", token.to_owned()))
        .body(body.to_string())
        .dispatch()
        .await;
    let status = response.status();
    let text = response.into_string().await.unwrap_or_default();
    let json = if text.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap()
    };
    (status, json)
}

fn actor_store_path(dir: &std::path::Path) -> std::path::PathBuf {
    let hash = hex::encode(Sha256::digest(DID.as_bytes()));
    dir.join("actors")
        .join(&hash[0..2])
        .join(DID)
        .join("store.sqlite")
}

fn post_body(rkey: &str, text: &str) -> Value {
    json!({
        "repo": DID,
        "collection": "app.bsky.feed.post",
        "rkey": rkey,
        "record": {
            "$type": "app.bsky.feed.post",
            "text": text,
            "createdAt": "2024-01-01T00:00:00.000Z",
        },
    })
}

#[tokio::test]
async fn writes_publish_once_and_a_restart_finishes_an_interrupted_delivery() {
    let dir = tempfile::tempdir().unwrap();
    let (identifier, password) = {
        let client = get_client_in(dir.path()).await;
        let (identifier, password) = create_account(&client).await;
        // the harness creates the account deactivated, as a migration does;
        // activate it at the row level, since the harness PLC document does
        // not carry the account's signing key
        rusqlite::Connection::open(dir.path().join("account.sqlite"))
            .unwrap()
            .execute(
                "UPDATE actor SET \"deactivatedAt\" = NULL, \"deleteAfter\" = NULL WHERE did = ?1",
                [DID],
            )
            .unwrap();
        let token = session_token(&client, &identifier, &password).await;
        let (status, json) = post(
            &client,
            "/xrpc/com.atproto.repo.createRecord",
            &token,
            post_body("3lfixtureaa2a", "first"),
        )
        .await;
        assert_eq!(status, Status::Ok, "{json}");
        let (status, json) = post(
            &client,
            "/xrpc/com.atproto.repo.putRecord",
            &token,
            post_body("3lfixtureaa2b", "second"),
        )
        .await;
        assert_eq!(status, Status::Ok, "{json}");
        let (status, json) = post(
            &client,
            "/xrpc/com.atproto.repo.deleteRecord",
            &token,
            json!({"repo": DID, "collection": "app.bsky.feed.post", "rkey": "3lfixtureaa2a"}),
        )
        .await;
        assert_eq!(status, Status::Ok, "{json}");
        // the reference limit applies through applyWrites too
        let writes: Vec<Value> = (0..201)
            .map(|i| {
                json!({
                    "$type": "com.atproto.repo.applyWrites#create",
                    "collection": "app.bsky.feed.post",
                    "rkey": format!("3lfixture{i:04}"),
                    "value": {
                        "$type": "app.bsky.feed.post",
                        "text": "n",
                        "createdAt": "2024-01-01T00:00:00.000Z",
                    },
                })
            })
            .collect();
        let (status, json) = post(
            &client,
            "/xrpc/com.atproto.repo.applyWrites",
            &token,
            json!({"repo": DID, "writes": writes}),
        )
        .await;
        assert_eq!(status, Status::BadRequest, "{json}");
        assert_eq!(json["error"], "InvalidRequest");
        assert_eq!(json["message"], "Too many writes. Max: 200");
        (identifier, password)
    };

    let sequencer = dir.path().join("sequencer.sqlite");
    let before = sequencer_events(&sequencer, DID);
    let types: Vec<&str> = before.iter().map(|(_, t, _)| t.as_str()).collect();
    assert_eq!(types, ["append", "append", "append"]);

    // a crash after the last row was sequenced but before the store
    // acknowledged it: the intent is still pending, the actor still marked
    let store = rusqlite::Connection::open(actor_store_path(dir.path())).unwrap();
    let (last_id, last_seq): (i64, i64) = store
        .query_row(
            "SELECT id, seq FROM publish_intent ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(last_seq, before.last().unwrap().0);
    store
        .execute(
            "UPDATE publish_intent SET state = 'pending', seq = NULL, \"seqFloor\" = ?1 WHERE id = ?2",
            rusqlite::params![last_seq - 1, last_id],
        )
        .unwrap();
    store
        .execute("DELETE FROM publish_ack WHERE \"intentId\" = ?1", [last_id])
        .unwrap();
    drop(store);
    let lifecycle = rusqlite::Connection::open(dir.path().join("rsky/lifecycle.sqlite")).unwrap();
    lifecycle
        .execute(
            "INSERT INTO pending_work (did, \"markedAt\") VALUES (?1, ?2)",
            rusqlite::params![DID, rsky_common::now()],
        )
        .unwrap();
    drop(lifecycle);

    let client = get_client_in(dir.path()).await;
    let after = sequencer_events(&sequencer, DID);
    assert_eq!(after, before, "the restart must not publish the row again");
    let store = rusqlite::Connection::open(actor_store_path(dir.path())).unwrap();
    let (state, seq): (String, i64) = store
        .query_row(
            "SELECT state, seq FROM publish_intent WHERE id = ?1",
            [last_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((state.as_str(), seq), ("delivered", last_seq));
    let lifecycle = rusqlite::Connection::open(dir.path().join("rsky/lifecycle.sqlite")).unwrap();
    let marked: i64 = lifecycle
        .query_row("SELECT count(*) FROM pending_work", [], |row| row.get(0))
        .unwrap();
    assert_eq!(marked, 0);

    // the restarted server keeps writing and publishing
    let token = session_token(&client, &identifier, &password).await;
    let (status, json) = post(
        &client,
        "/xrpc/com.atproto.repo.createRecord",
        &token,
        post_body("3lfixtureaa2c", "third"),
    )
    .await;
    assert_eq!(status, Status::Ok, "{json}");
    assert_eq!(sequencer_events(&sequencer, DID).len(), before.len() + 1);

    // a blob referenced by a record is served until a moderator takes it
    // down, and again once the takedown is reversed
    // a one-pixel PNG, since an image embed accepts only images
    let png: Vec<u8> = vec![
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8,
        0xcf, 0xc0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0xc9, 0xfe, 0x92, 0xef, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    let upload = client
        .post("/xrpc/com.atproto.repo.uploadBlob")
        .header(ContentType::PNG)
        .header(Header::new("Authorization", token.clone()))
        .body(png)
        .dispatch()
        .await;
    assert_eq!(upload.status(), Status::Ok);
    let blob: Value = upload.into_json().await.unwrap();
    let blob_cid = blob["blob"]["ref"]["$link"].as_str().unwrap().to_owned();
    let (status, json) = post(
        &client,
        "/xrpc/com.atproto.repo.createRecord",
        &token,
        json!({
            "repo": DID,
            "collection": "app.bsky.feed.post",
            "rkey": "3lfixtureaa2d",
            "record": {
                "$type": "app.bsky.feed.post",
                "text": "with blob",
                "createdAt": "2024-01-01T00:00:00.000Z",
                "embed": {
                    "$type": "app.bsky.embed.images",
                    "images": [{"alt": "", "image": blob["blob"]}],
                },
            },
        }),
    )
    .await;
    assert_eq!(status, Status::Ok, "{json}");
    let get_blob = || {
        client
            .get(format!(
                "/xrpc/com.atproto.sync.getBlob?did={DID}&cid={blob_cid}"
            ))
            .dispatch()
    };
    assert_eq!(get_blob().await.status(), Status::Ok);
    for applied in [true, false] {
        let (status, json) = post(
            &client,
            "/xrpc/com.atproto.admin.updateSubjectStatus",
            &get_admin_token(),
            json!({
                "subject": {
                    "$type": "com.atproto.admin.defs#repoBlobRef",
                    "did": DID,
                    "cid": blob_cid,
                },
                "takedown": {"applied": applied, "ref": "mod-1"},
            }),
        )
        .await;
        assert_eq!(status, Status::Ok, "{json}");
        let expected = if applied {
            Status::BadRequest
        } else {
            Status::Ok
        };
        assert_eq!(get_blob().await.status(), expected);
    }
}
