//! The write allowlist through the public routes: an actor the file does not
//! name is refused, a named actor is served, a change to the file takes
//! effect without a restart, and the drain status reports the actor's state.

mod common;

use common::{create_account, get_admin_token, get_client_in};
use rocket::http::{ContentType, Header, Status};
use rocket::local::asynchronous::Client;
use rocket::serde::json::json;
use serde_json::Value;
use std::time::{Duration, SystemTime};

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

async fn create_record(
    client: &Client,
    token: &str,
    rkey: &str,
) -> (Status, Value, Option<String>) {
    let response = client
        .post("/xrpc/com.atproto.repo.createRecord")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", token.to_owned()))
        .body(
            json!({
                "repo": DID,
                "collection": "app.bsky.feed.post",
                "rkey": rkey,
                "record": {
                    "$type": "app.bsky.feed.post",
                    "text": "gated",
                    "createdAt": "2024-01-01T00:00:00.000Z",
                },
            })
            .to_string(),
        )
        .dispatch()
        .await;
    let status = response.status();
    let retry_after = response.headers().get_one("Retry-After").map(str::to_owned);
    let json: Value = response.into_json().await.unwrap_or(Value::Null);
    (status, json, retry_after)
}

async fn drain_status(client: &Client, authorized: bool) -> (Status, Value) {
    let mut request = client.get(format!("/xrpc/_drain_status?did={DID}"));
    if authorized {
        request = request.header(Header::new("Authorization", get_admin_token()));
    }
    let response = request.dispatch().await;
    let status = response.status();
    let json: Value = response.into_json().await.unwrap_or(Value::Null);
    (status, json)
}

fn write_allowlist(path: &std::path::Path, text: &str, offset: Duration) {
    std::fs::write(path, text).unwrap();
    // the reloader watches the modification time; make the change unambiguous
    std::fs::File::open(path)
        .unwrap()
        .set_modified(SystemTime::now() + offset)
        .unwrap();
}

#[tokio::test]
async fn the_allowlist_gates_writes_and_reloads_without_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let allowlist = dir.path().join("write-allowlist.toml");
    write_allowlist(
        &allowlist,
        &format!("version = 1\ndefault = \"absent\"\n[entries]\n\"{DID}\" = \"active\"\n"),
        Duration::ZERO,
    );
    std::env::set_var("PDS_WRITE_ALLOWLIST_FILE", &allowlist);
    std::env::set_var("PDS_LOCK_DIR", dir.path().join("rsky/locks"));
    let client = get_client_in(dir.path()).await;
    let (identifier, password) = create_account(&client).await;
    rusqlite::Connection::open(dir.path().join("account.sqlite"))
        .unwrap()
        .execute(
            "UPDATE actor SET \"deactivatedAt\" = NULL, \"deleteAfter\" = NULL WHERE did = ?1",
            [DID],
        )
        .unwrap();
    let token = session_token(&client, &identifier, &password).await;

    let (status, json, _) = create_record(&client, &token, "3lfixtureaa2a").await;
    assert_eq!(status, Status::Ok, "{json}");
    assert!(dir
        .path()
        .join("rsky/locks")
        .join(format!("{DID}.lock"))
        .is_file());

    let (status, json) = drain_status(&client, true).await;
    assert_eq!(status, Status::Ok, "{json}");
    assert_eq!(json["did"], DID);
    assert_eq!(json["state"], "active");
    assert_eq!(json["fullyDrained"], true);
    assert_eq!(json["inflightMutations"], 0);
    let (status, _) = drain_status(&client, false).await;
    assert_ne!(status, Status::Ok);

    // an account the file does not name cannot even be created
    let invite = client
        .post("/xrpc/com.atproto.server.createInviteCode")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", get_admin_token()))
        .body(json!({"useCount": 1}).to_string())
        .dispatch()
        .await
        .into_json::<Value>()
        .await
        .unwrap()["code"]
        .as_str()
        .unwrap()
        .to_owned();
    let response = client
        .post("/xrpc/com.atproto.server.createAccount")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", get_admin_token()))
        .body(
            json!({
                "did": "did:plc:aaaaaaaaaaaaaaaaaaaaaaaa",
                "email": "other@example.com",
                "handle": "zzgated.rsky.com",
                "password": "password",
                "inviteCode": invite,
            })
            .to_string(),
        )
        .dispatch()
        .await;
    let status = response.status();
    let body: Value = response.into_json().await.unwrap();
    assert_eq!(status, Status::ServiceUnavailable, "{body}");
    assert_eq!(body["error"], "NotAdmitted", "{body}");

    // draining the actor takes effect on the next reload
    write_allowlist(
        &allowlist,
        &format!("version = 1\ndefault = \"absent\"\n[entries]\n\"{DID}\" = \"draining\"\n"),
        Duration::from_secs(5),
    );
    let mut refused = None;
    for attempt in 0..60 {
        let rkey = format!("3lfixtureb{attempt:03}");
        let (status, json, retry_after) = create_record(&client, &token, &rkey).await;
        if status == Status::ServiceUnavailable {
            refused = Some((json, retry_after));
            break;
        }
        assert_eq!(status, Status::Ok, "{json}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let (json, retry_after) = refused.expect("the reloaded allowlist refuses the write");
    assert_eq!(json["error"], "NotAdmitted");
    assert!(json["message"].as_str().unwrap().contains("draining"));
    assert_eq!(retry_after.as_deref(), Some("1"));
    let (status, json) = drain_status(&client, true).await;
    assert_eq!(status, Status::Ok);
    assert_eq!(json["state"], "draining");
    assert_eq!(json["fullyDrained"], true);

    // the maintenance drain finishes the actor's work over the same files
    for (key, name) in [
        ("PDS_ACTOR_STORE_DIRECTORY", "actors"),
        ("PDS_ACCOUNT_DB_LOCATION", "account.sqlite"),
        ("PDS_SEQUENCER_DB_LOCATION", "sequencer.sqlite"),
        ("PDS_DID_CACHE_DB_LOCATION", "did_cache.sqlite"),
        ("PDS_LIFECYCLE_DB", "rsky/lifecycle.sqlite"),
    ] {
        std::env::set_var(key, dir.path().join(name));
    }
    let drained = rsky_pds::drain::run_from_env(rsky_pds::drain::DrainArgs {
        did: DID.to_owned(),
        timeout: Duration::from_secs(5),
    })
    .await
    .unwrap();
    assert_eq!(drained.state, "draining");
    assert!(drained.fully_drained);

    // without an allowlist every actor is admitted
    std::env::remove_var("PDS_WRITE_ALLOWLIST_FILE");
    let open = rsky_pds::drain::run_from_env(rsky_pds::drain::DrainArgs {
        did: DID.to_owned(),
        timeout: Duration::from_secs(5),
    })
    .await
    .unwrap();
    assert_eq!(open.state, "active");
    assert!(open.fully_drained);
}
