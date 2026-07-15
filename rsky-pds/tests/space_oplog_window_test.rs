//! Runs in its own test binary (separate process) because it configures the
//! oplog retention window through the process environment.

use rocket::http::{ContentType, Header, Status};
use rsky_pds::account_manager::AccountManager;
use rsky_pds::config::ServerConfig;
use serde_json::{json, Value};

mod common;

const DID: &str = "did:plc:oplogwindowaaaaaaaaaaaaa";
const SPACE_TYPE: &str = "com.example.forum";
const COLLECTION: &str = "com.example.post";

#[tokio::test]
async fn compacted_history_is_unavailable_over_the_wire() {
    std::env::set_var("PDS_SPACE_OPLOG_WINDOW", "2");
    let (_dir, client) = common::get_client().await;

    // account setup
    let domain = client
        .rocket()
        .state::<ServerConfig>()
        .unwrap()
        .identity
        .service_handle_domains
        .first()
        .unwrap()
        .clone();
    let invite = client
        .post("/xrpc/com.atproto.server.createInviteCode")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", common::get_admin_token()))
        .body(json!({"useCount": 1}).to_string())
        .dispatch()
        .await
        .into_json::<Value>()
        .await
        .unwrap()["code"]
        .as_str()
        .unwrap()
        .to_string();
    let response = client
        .post("/xrpc/com.atproto.server.createAccount")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", common::get_admin_token()))
        .body(
            json!({
                "did": DID,
                "email": "oplog@example.com",
                "handle": format!("oplog{domain}"),
                "password": "password",
                "inviteCode": invite
            })
            .to_string(),
        )
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    client
        .rocket()
        .state::<AccountManager>()
        .unwrap()
        .activate_account(DID)
        .await
        .unwrap();
    let token = client
        .post("/xrpc/com.atproto.server.createSession")
        .header(ContentType::JSON)
        .body(json!({"identifier": format!("oplog{domain}"), "password": "password"}).to_string())
        .dispatch()
        .await
        .into_json::<Value>()
        .await
        .unwrap()["accessJwt"]
        .as_str()
        .unwrap()
        .to_string();
    let auth = Header::new("Authorization", format!("Bearer {token}"));

    let space = format!("at://{DID}/space/{SPACE_TYPE}/self");
    let mut revs: Vec<String> = Vec::new();
    for i in 0..5 {
        let response = client
            .post("/xrpc/com.atproto.space.createRecord")
            .header(ContentType::JSON)
            .header(auth.clone())
            .body(
                json!({
                    "space": space,
                    "collection": COLLECTION,
                    "rkey": format!("3k{i}"),
                    "record": {"text": format!("post {i}")}
                })
                .to_string(),
            )
            .dispatch()
            .await;
        assert_eq!(response.status(), Status::Ok);
        let body: Value = response.into_json().await.unwrap();
        revs.push(body["commit"]["rev"].as_str().unwrap().to_string());
    }

    // full-history requests fail once compaction has dropped revisions
    let response = client
        .get(format!(
            "/xrpc/com.atproto.space.listRepoOps?space={space}&did={DID}"
        ))
        .header(auth.clone())
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::BadRequest);
    let body: Value = response.into_json().await.unwrap();
    assert_eq!(body["error"], "HistoryUnavailable");

    // requests below the floor fail too
    let response = client
        .get(format!(
            "/xrpc/com.atproto.space.listRepoOps?space={space}&did={DID}&since={}",
            revs[0]
        ))
        .header(auth.clone())
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::BadRequest);

    // requests at or after the floor are served with the current commit
    let response = client
        .get(format!(
            "/xrpc/com.atproto.space.listRepoOps?space={space}&did={DID}&since={}",
            revs[3]
        ))
        .header(auth)
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: Value = response.into_json().await.unwrap();
    assert_eq!(body["ops"].as_array().unwrap().len(), 1);
    assert!(body.get("commit").is_some());
    std::env::remove_var("PDS_SPACE_OPLOG_WINDOW");
}
