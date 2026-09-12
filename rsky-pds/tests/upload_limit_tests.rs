//! Uploads are spooled to disk, refused one byte past `PDS_BLOB_UPLOAD_LIMIT`,
//! and streamed back with the length their registration recorded.

mod common;

use common::{create_account, get_client};
use rocket::http::{ContentType, Header, Status};
use rocket::local::asynchronous::Client;
use serde_json::{json, Value};

const DID: &str = "did:plc:khvyd3oiw46vif5gm7hijslk";
const LIMIT: usize = 1024;

fn png_of(len: usize) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.resize(len, 7);
    bytes
}

async fn upload(client: &Client, token: &str, bytes: Vec<u8>) -> (Status, Value) {
    let response = client
        .post("/xrpc/com.atproto.repo.uploadBlob")
        .header(ContentType::PNG)
        .header(Header::new("Authorization", token.to_owned()))
        .body(bytes)
        .dispatch()
        .await;
    let status = response.status();
    (status, response.into_json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn uploads_are_bounded_spooled_and_streamed_back() {
    let spool = tempfile::tempdir().unwrap();
    std::env::set_var("PDS_BLOB_UPLOAD_LIMIT", LIMIT.to_string());
    std::env::set_var("PDS_UPLOAD_SPOOL_DIR", spool.path());
    let (dir, client) = get_client().await;
    let (identifier, password) = create_account(&client).await;
    rusqlite::Connection::open(dir.path().join("account.sqlite"))
        .unwrap()
        .execute(
            "UPDATE actor SET \"deactivatedAt\" = NULL, \"deleteAfter\" = NULL WHERE did = ?1",
            [DID],
        )
        .unwrap();
    let session = client
        .post("/xrpc/com.atproto.server.createSession")
        .header(ContentType::JSON)
        .body(json!({"identifier": identifier, "password": password}).to_string())
        .dispatch()
        .await;
    let session: Value = session.into_json().await.unwrap();
    let token = format!("Bearer {}", session["accessJwt"].as_str().unwrap());

    let (status, body) = upload(&client, &token, png_of(LIMIT + 1)).await;
    assert_eq!(status, Status::PayloadTooLarge, "{body}");
    assert_eq!(body["error"], "PayloadTooLarge");
    assert_eq!(body["message"], "request entity too large");
    let (status, body) = upload(&client, &token, png_of(LIMIT - 1)).await;
    assert_eq!(status, Status::Ok, "{body}");
    assert_eq!(body["blob"]["size"], LIMIT - 1);
    let (status, body) = upload(&client, &token, png_of(LIMIT)).await;
    assert_eq!(status, Status::Ok, "{body}");
    assert_eq!(body["blob"]["size"], LIMIT);
    assert_eq!(body["blob"]["mimeType"], "image/png");
    let blob = body["blob"].clone();
    // nothing is left in the spool once each request is answered
    assert_eq!(std::fs::read_dir(spool.path()).unwrap().count(), 0);

    // referenced by a record, the blob streams back with its length
    let created = client
        .post("/xrpc/com.atproto.repo.createRecord")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", token.clone()))
        .body(
            json!({
                "repo": DID,
                "collection": "app.bsky.feed.post",
                "record": {
                    "$type": "app.bsky.feed.post",
                    "text": "with an image",
                    "createdAt": "2024-01-01T00:00:00.000Z",
                    "embed": {
                        "$type": "app.bsky.embed.images",
                        "images": [{"image": blob, "alt": "padding"}]
                    }
                },
            })
            .to_string(),
        )
        .dispatch()
        .await;
    assert_eq!(created.status(), Status::Ok);
    let cid = blob["ref"]["$link"].as_str().unwrap();
    let response = client
        .get(format!(
            "/xrpc/com.atproto.sync.getBlob?did={DID}&cid={cid}"
        ))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    assert_eq!(
        response.headers().get_one("content-type"),
        Some("image/png")
    );
    assert_eq!(response.into_bytes().await.unwrap(), png_of(LIMIT));
}
