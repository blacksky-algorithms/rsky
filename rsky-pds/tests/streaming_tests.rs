//! Large reads over a real socket: a blob arrives with the length its
//! registration recorded, and a repository export is sent as it is
//! produced.

mod common;

use common::{create_account, get_client_in, pds_binary};
use rocket::http::{ContentType, Header};
use serde_json::{json, Value};
use std::net::TcpListener;
use std::process::Stdio;
use std::time::{Duration, Instant};

const DID: &str = "did:plc:khvyd3oiw46vif5gm7hijslk";

#[tokio::test]
async fn blobs_and_exports_stream_over_http() {
    let dir = tempfile::tempdir().unwrap();
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.resize(300_000, 9);
    let blob = {
        let client = get_client_in(dir.path()).await;
        let (identifier, password) = create_account(&client).await;
        rusqlite::Connection::open(dir.path().join("account.sqlite"))
            .unwrap()
            .execute(
                "UPDATE actor SET \"deactivatedAt\" = NULL, \"deleteAfter\" = NULL WHERE did = ?1",
                [DID],
            )
            .unwrap();
        let session: Value = client
            .post("/xrpc/com.atproto.server.createSession")
            .header(ContentType::JSON)
            .body(json!({"identifier": identifier, "password": password}).to_string())
            .dispatch()
            .await
            .into_json()
            .await
            .unwrap();
        let token = format!("Bearer {}", session["accessJwt"].as_str().unwrap());
        let uploaded: Value = client
            .post("/xrpc/com.atproto.repo.uploadBlob")
            .header(ContentType::PNG)
            .header(Header::new("Authorization", token.clone()))
            .body(png.clone())
            .dispatch()
            .await
            .into_json()
            .await
            .unwrap();
        let blob = uploaded["blob"].clone();
        let created = client
            .post("/xrpc/com.atproto.repo.createRecord")
            .header(ContentType::JSON)
            .header(Header::new("Authorization", token))
            .body(
                json!({
                    "repo": DID,
                    "collection": "app.bsky.feed.post",
                    "record": {
                        "$type": "app.bsky.feed.post",
                        "text": "streamed",
                        "createdAt": "2024-01-01T00:00:00.000Z",
                        "embed": {"$type": "app.bsky.embed.images", "images": [{"image": blob, "alt": ""}]}
                    },
                })
                .to_string(),
            )
            .dispatch()
            .await;
        assert_eq!(created.status().code, 200);
        blob
    };

    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut child = pds_binary(dir.path())
        .env("PDS_PORT", port.to_string())
        .env(
            "PDS_BLOBSTORE_DISK_LOCATION",
            std::env::var("PDS_BLOBSTORE_DISK_LOCATION").unwrap(),
        )
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let base = format!("http://127.0.0.1:{port}");
    let started = Instant::now();
    loop {
        if reqwest::get(format!("{base}/xrpc/_health/live"))
            .await
            .is_ok_and(|res| res.status() == 200)
        {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "server did not come up"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let cid = blob["ref"]["$link"].as_str().unwrap();
    let response = reqwest::get(format!(
        "{base}/xrpc/com.atproto.sync.getBlob?did={DID}&cid={cid}"
    ))
    .await
    .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers().get("content-length").unwrap(),
        &png.len().to_string()
    );
    assert_eq!(response.headers().get("content-type").unwrap(), "image/png");
    assert_eq!(response.bytes().await.unwrap().to_vec(), png);

    let response = reqwest::get(format!("{base}/xrpc/com.atproto.sync.getRepo?did={DID}"))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/vnd.ipld.car"
    );
    assert_eq!(
        response.headers().get("transfer-encoding").unwrap(),
        "chunked"
    );
    let car = response.bytes().await.unwrap().to_vec();
    let parsed = rsky_repo::car::read_car_with_root(car).await.unwrap();
    assert!(parsed.blocks.map.len() > 1);

    // SAFETY: the pid names the child spawned above
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let status = child.wait().unwrap();
    assert!(status.success(), "{status:?}");
}
