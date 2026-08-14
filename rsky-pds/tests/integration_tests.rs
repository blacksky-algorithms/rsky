use crate::common::{create_account, get_admin_token};
use rocket::http::{ContentType, Header, Status};
use rsky_lexicon::com::atproto::server::CreateInviteCodeOutput;
use rsky_pds::config::ServerConfig;
use serde_json::json;

mod common;

#[tokio::test]
async fn test_index() {
    let (_dir, client) = common::get_client().await;
    let response = client.get("/").dispatch().await;
    assert_eq!(response.status(), Status::Ok);
}

#[tokio::test]
async fn test_health() {
    let (_dir, client) = common::get_client().await;
    let response = client.get("/xrpc/_health").dispatch().await;
    assert_eq!(response.status(), Status::Ok);
}

#[tokio::test]
async fn test_robots_txt() {
    let (_dir, client) = common::get_client().await;
    let response = client.get("/robots.txt").dispatch().await;
    let response_status = response.status();
    let response_body = response.into_string().await.unwrap();
    assert_eq!(response_status, Status::Ok);
    assert_eq!(
        response_body,
        "# Hello!\n\n# Crawling the public API is allowed\nUser-agent: *\nAllow: /"
    );
}

#[tokio::test]
async fn test_create_invite_code() {
    let (_dir, client) = common::get_client().await;
    let input = json!({
        "useCount": 1
    });

    let response = client
        .post("/xrpc/com.atproto.server.createInviteCode")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", get_admin_token()))
        .body(input.to_string())
        .dispatch()
        .await;
    let response_status = response.status();

    assert_eq!(response_status, Status::Ok);
    response
        .into_json::<CreateInviteCodeOutput>()
        .await
        .unwrap();
}

#[tokio::test]
async fn test_create_invite_code_and_account() {
    let (_dir, client) = common::get_client().await;
    let domain = client
        .rocket()
        .state::<ServerConfig>()
        .unwrap()
        .identity
        .service_handle_domains
        .first()
        .unwrap();

    let input = json!({
        "useCount": 1
    });

    let response = client
        .post("/xrpc/com.atproto.server.createInviteCode")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", get_admin_token()))
        .body(input.to_string())
        .dispatch()
        .await;
    let invite_code = response
        .into_json::<CreateInviteCodeOutput>()
        .await
        .unwrap()
        .code;

    let account_input = json!({
        "did": "did:plc:khvyd3oiw46vif5gm7hijslk",
        "email": "foo@example.com",
        "handle": format!("foo{domain}"),
        "password": "password",
        "inviteCode": invite_code
    });

    let response = client
        .post("/xrpc/com.atproto.server.createAccount")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", get_admin_token()))
        .body(account_input.to_string())
        .dispatch()
        .await;
    let response_status = response.status();
    assert_eq!(response_status, Status::Ok);
}

#[tokio::test]
async fn test_create_session() {
    let (_dir, client) = common::get_client().await;

    let (username, password) = create_account(&client).await;

    // Valid Login
    let session_input = json!({
        "identifier": username,
        "password": password,
    });

    let response = client
        .post("/xrpc/com.atproto.server.createSession")
        .header(ContentType::JSON)
        .body(session_input.to_string())
        .dispatch()
        .await;
    let response_status = response.status();
    assert_eq!(response_status, Status::Ok);

    // Invalid Login
    let session_input = json!({
        "identifier": username,
        "password": password + "1",
    });

    let response = client
        .post("/xrpc/com.atproto.server.createSession")
        .header(ContentType::JSON)
        .body(session_input.to_string())
        .dispatch()
        .await;
    let response_status = response.status();
    assert_eq!(response_status, Status::BadRequest);
}

#[tokio::test]
async fn test_list_repos() {
    let (_dir, client) = common::get_client().await;
    create_account(&client).await;

    let response = client
        .get("/xrpc/com.atproto.sync.listRepos?limit=10")
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: serde_json::Value = response.into_json().await.unwrap();
    assert_eq!(body["repos"].as_array().unwrap().len(), 1);
    assert_eq!(body["repos"][0]["did"], "did:plc:khvyd3oiw46vif5gm7hijslk");
}

#[tokio::test]
async fn test_get_invite_codes() {
    let (_dir, client) = common::get_client().await;
    create_account(&client).await;

    for sort in ["recent", "usage"] {
        let response = client
            .get(format!(
                "/xrpc/com.atproto.admin.getInviteCodes?sort={sort}&limit=10"
            ))
            .header(Header::new("Authorization", get_admin_token()))
            .dispatch()
            .await;
        assert_eq!(response.status(), Status::Ok);
        let body: serde_json::Value = response.into_json().await.unwrap();
        assert_eq!(body["codes"].as_array().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn test_liveness_options_and_catcher() {
    let (_dir, client) = common::get_client().await;

    let response = client.get("/xrpc/_health/live").dispatch().await;
    assert_eq!(response.status(), Status::Ok);
    assert_eq!(response.into_string().await.unwrap(), "ok");

    // CORS headers are attached to preflight responses by the fairing
    let response = client
        .options("/xrpc/com.atproto.server.createSession")
        .dispatch()
        .await;
    assert_eq!(
        response.headers().get_one("Access-Control-Allow-Origin"),
        Some("*")
    );

    // unhandled paths fall through to the default catcher
    let response = client.get("/does-not-exist").dispatch().await;
    assert_eq!(response.status(), Status::InternalServerError);
}

async fn get_access_token(client: &rocket::local::asynchronous::Client) -> String {
    let (username, password) = create_account(client).await;
    let session_input = json!({
        "identifier": username,
        "password": password,
    });
    let response = client
        .post("/xrpc/com.atproto.server.createSession")
        .header(ContentType::JSON)
        .body(session_input.to_string())
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: serde_json::Value = response.into_json().await.unwrap();
    body["accessJwt"].as_str().unwrap().to_string()
}

/// `listRecords` must return the record value itself and omit the cursor on a
/// short final page. Consumers treat a nested `{uri, cid, value}` as a failed
/// write, and a final-page cursor invites a needless empty request.
#[tokio::test]
async fn test_list_records_has_direct_values_and_no_final_cursor() {
    let (_dir, client) = common::get_client().await;
    let token = get_access_token(&client).await;
    let did = "did:plc:khvyd3oiw46vif5gm7hijslk";
    // Accounts created with a supplied did start deactivated; writes need it live.
    client
        .rocket()
        .state::<rsky_pds::account_manager::AccountManager>()
        .unwrap()
        .activate_account(did)
        .await
        .unwrap();

    for rkey in ["one", "two"] {
        let response = client
            .post("/xrpc/com.atproto.repo.createRecord")
            .header(ContentType::JSON)
            .header(Header::new("Authorization", format!("Bearer {token}")))
            .body(
                json!({
                    "repo": did,
                    "collection": "com.example.record",
                    "rkey": rkey,
                    "validate": false,
                    "record": {"$type": "com.example.record", "text": rkey}
                })
                .to_string(),
            )
            .dispatch()
            .await;
        assert_eq!(response.status(), Status::Ok, "create {rkey}");
    }

    let response = client
        .get(format!(
            "/xrpc/com.atproto.repo.listRecords?repo={did}&collection=com.example.record&limit=1"
        ))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let first: serde_json::Value = response.into_json().await.unwrap();
    assert_eq!(first["records"].as_array().unwrap().len(), 1);
    assert_eq!(first["records"][0]["value"]["$type"], "com.example.record");
    assert!(first["records"][0]["value"].get("value").is_none());
    let cursor = first["cursor"].as_str().expect("full page has a cursor");

    let response = client
        .get(format!(
            "/xrpc/com.atproto.repo.listRecords?repo={did}&collection=com.example.record&limit=1&cursor={cursor}"
        ))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let final_page: serde_json::Value = response.into_json().await.unwrap();
    assert_eq!(final_page["records"].as_array().unwrap().len(), 1);
    assert!(final_page.get("cursor").is_none(), "{final_page}");
}

#[tokio::test]
async fn test_check_signup_queue() {
    let (_dir, client) = common::get_client().await;
    let token = get_access_token(&client).await;

    let response = client
        .get("/xrpc/com.atproto.temp.checkSignupQueue")
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: serde_json::Value = response.into_json().await.unwrap();
    assert_eq!(body["activated"], true);
    assert!(body.get("placeInQueue").is_none());

    // requires auth
    let response = client
        .get("/xrpc/com.atproto.temp.checkSignupQueue")
        .dispatch()
        .await;
    assert_ne!(response.status(), Status::Ok);
}

#[tokio::test]
async fn test_get_head_and_get_checkout() {
    let (_dir, client) = common::get_client().await;
    create_account(&client).await;
    let did = "did:plc:khvyd3oiw46vif5gm7hijslk";

    // accounts created with a supplied did start deactivated
    client
        .rocket()
        .state::<rsky_pds::account_manager::AccountManager>()
        .unwrap()
        .activate_account(did)
        .await
        .unwrap();

    let response = client
        .get(format!("/xrpc/com.atproto.sync.getHead?did={did}"))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: serde_json::Value = response.into_json().await.unwrap();
    assert!(!body["root"].as_str().unwrap().is_empty());

    let response = client
        .get("/xrpc/com.atproto.sync.getHead?did=did:plc:doesnotexist")
        .dispatch()
        .await;
    assert_ne!(response.status(), Status::Ok);

    let response = client
        .get(format!("/xrpc/com.atproto.sync.getCheckout?did={did}"))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    assert_eq!(
        response.headers().get_one("Content-Type"),
        Some("application/vnd.ipld.car")
    );
    assert!(!response.into_bytes().await.unwrap().is_empty());

    let response = client
        .get("/xrpc/com.atproto.sync.getCheckout?did=did:plc:doesnotexist")
        .dispatch()
        .await;
    assert_ne!(response.status(), Status::Ok);
}

#[tokio::test]
async fn test_get_account_infos() {
    let (_dir, client) = common::get_client().await;
    create_account(&client).await;

    let response = client
        .get("/xrpc/com.atproto.admin.getAccountInfos?dids=did:plc:khvyd3oiw46vif5gm7hijslk&dids=did:plc:doesnotexist")
        .header(Header::new("Authorization", get_admin_token()))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: serde_json::Value = response.into_json().await.unwrap();
    let infos = body["infos"].as_array().unwrap();
    // missing accounts are skipped
    assert_eq!(infos.len(), 1);
    assert_eq!(infos[0]["did"], "did:plc:khvyd3oiw46vif5gm7hijslk");

    // requires moderator auth
    let response = client
        .get("/xrpc/com.atproto.admin.getAccountInfos?dids=did:plc:khvyd3oiw46vif5gm7hijslk")
        .dispatch()
        .await;
    assert_ne!(response.status(), Status::Ok);
}

#[tokio::test]
async fn test_create_report() {
    let (_dir, client) = common::get_client().await;
    let token = get_access_token(&client).await;

    // reasonType must not be empty
    let response = client
        .post("/xrpc/com.atproto.moderation.createReport")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .body(
            json!({
                "reasonType": " ",
                "subject": {
                    "$type": "com.atproto.admin.defs#repoRef",
                    "did": "did:plc:khvyd3oiw46vif5gm7hijslk"
                }
            })
            .to_string(),
        )
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::BadRequest);
    let body: serde_json::Value = response.into_json().await.unwrap();
    assert_eq!(body["error"], "InvalidRequest");

    // the pinned test report service is a localhost url, which the proxy
    // rejects before any request is made
    let response = client
        .post("/xrpc/com.atproto.moderation.createReport")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .body(
            json!({
                "reasonType": "com.atproto.moderation.defs#reasonSpam",
                "reason": "spam",
                "subject": {
                    "$type": "com.atproto.repo.strongRef",
                    "uri": "at://did:plc:khvyd3oiw46vif5gm7hijslk/app.bsky.feed.post/abc",
                    "cid": "bafkreibjfgx2gprinfvicegelk5kosd6y2frmqpqzwqkg7usac74l3t2v4"
                }
            })
            .to_string(),
        )
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::BadRequest);
    let body: serde_json::Value = response.into_json().await.unwrap();
    assert_eq!(body["error"], "InvalidRequest");
}

#[tokio::test]
async fn test_unregister_push() {
    let (_dir, client) = common::get_client().await;
    let token = get_access_token(&client).await;

    // proxied to the configured (mock) appview
    let response = client
        .post("/xrpc/app.bsky.notification.unregisterPush")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .body(
            json!({
                "serviceDid": "did:web:appview.invalid",
                "token": "push-token",
                "platform": "web",
                "appId": "xyz.blueskyweb.app"
            })
            .to_string(),
        )
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);

    let response = client
        .post("/xrpc/app.bsky.notification.unregisterPush")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .body(
            json!({
                "serviceDid": "did:web:api.example.com",
                "token": "push-token",
                "platform": "blackberry",
                "appId": "xyz.blueskyweb.app"
            })
            .to_string(),
        )
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::BadRequest);
}

#[tokio::test]
async fn test_identity_resolution() {
    let (_dir, client) = common::get_client().await;
    create_account(&client).await;
    let did = "did:plc:khvyd3oiw46vif5gm7hijslk";
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

    // local handle lookups only see active accounts
    client
        .rocket()
        .state::<rsky_pds::account_manager::AccountManager>()
        .unwrap()
        .activate_account(did)
        .await
        .unwrap();

    let response = client
        .get(format!("/xrpc/com.atproto.identity.resolveDid?did={did}"))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: serde_json::Value = response.into_json().await.unwrap();
    assert_eq!(body["didDoc"]["id"], did);

    // DID input: handle bi-directionally verified via the local account store
    let response = client
        .get(format!(
            "/xrpc/com.atproto.identity.resolveIdentity?identifier={did}"
        ))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: serde_json::Value = response.into_json().await.unwrap();
    assert_eq!(body["did"], did);
    assert_eq!(body["handle"], handle.as_str());
    assert_eq!(body["didDoc"]["id"], did);

    // handle input resolves to the same identity
    let response = client
        .get(format!(
            "/xrpc/com.atproto.identity.resolveIdentity?identifier={handle}"
        ))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: serde_json::Value = response.into_json().await.unwrap();
    assert_eq!(body["did"], did);
    assert_eq!(body["handle"], handle.as_str());

    // refreshIdentity bypasses the cache and returns the same identity
    let response = client
        .post("/xrpc/com.atproto.identity.refreshIdentity")
        .header(ContentType::JSON)
        .body(json!({ "identifier": did }).to_string())
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: serde_json::Value = response.into_json().await.unwrap();
    assert_eq!(body["did"], did);
    assert_eq!(body["handle"], handle.as_str());

    // unsupported DID methods resolve to DidNotFound
    let response = client
        .get("/xrpc/com.atproto.identity.resolveDid?did=did:example:abc")
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::BadRequest);
    let body: serde_json::Value = response.into_json().await.unwrap();
    assert_eq!(body["error"], "DidNotFound");

    let response = client
        .get("/xrpc/com.atproto.identity.resolveIdentity?identifier=did:example:abc")
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::BadRequest);
    let body: serde_json::Value = response.into_json().await.unwrap();
    assert_eq!(body["error"], "DidNotFound");

    // a second account whose handle is not claimed by its (mock) DID document
    // fails bi-directional verification in both directions
    let bar_did = "did:plc:aaaaaaaaaaaaaaaaaaaaaaaa";
    let bar_handle = format!("bar{domain}");
    let invite = client
        .post("/xrpc/com.atproto.server.createInviteCode")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", get_admin_token()))
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
        .header(Header::new("Authorization", get_admin_token()))
        .body(
            json!({
                "did": bar_did,
                "email": "bar@example.com",
                "handle": bar_handle,
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
        .state::<rsky_pds::account_manager::AccountManager>()
        .unwrap()
        .activate_account(bar_did)
        .await
        .unwrap();

    let response = client
        .get(format!(
            "/xrpc/com.atproto.identity.resolveIdentity?identifier={bar_did}"
        ))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: serde_json::Value = response.into_json().await.unwrap();
    assert_eq!(body["did"], bar_did);
    assert_eq!(body["handle"], "handle.invalid");

    let response = client
        .get(format!(
            "/xrpc/com.atproto.identity.resolveIdentity?identifier={bar_handle}"
        ))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: serde_json::Value = response.into_json().await.unwrap();
    assert_eq!(body["did"], bar_did);
    assert_eq!(body["handle"], "handle.invalid");
}

/// An authorization server resolving an `include:` permission set reaches for
/// this method, unauthenticated, and needs a proof either way: absence is
/// answered with a CAR that proves it, not with an error. Its absence on a PDS
/// surfaces two hops away as an opaque `invalid_scope`, naming neither the
/// method nor the host.
#[tokio::test]
async fn test_sync_get_record_proves_presence_and_absence_anonymously() {
    let (_dir, client) = common::get_client().await;
    create_account(&client).await;
    let did = "did:plc:khvyd3oiw46vif5gm7hijslk";
    client
        .rocket()
        .state::<rsky_pds::account_manager::AccountManager>()
        .unwrap()
        .activate_account(did)
        .await
        .unwrap();

    for rkey in ["self", "nothing-was-ever-written-here"] {
        let response = client
            .get(format!(
                "/xrpc/com.atproto.sync.getRecord?did={did}&collection=com.atproto.lexicon.schema&rkey={rkey}"
            ))
            .dispatch()
            .await;
        assert_eq!(response.status(), Status::Ok, "rkey {rkey}");
        assert_eq!(
            response.headers().get_one("Content-Type"),
            Some("application/vnd.ipld.car")
        );
        assert!(
            !response.into_bytes().await.unwrap().is_empty(),
            "the CAR carries the blocks that prove the answer"
        );
    }
}

/// applyWrites must answer with a JSON object {commit, results}, not an empty
/// 200. A client that requires JSON reads an empty body as a failed write and
/// retries, duplicating records — the bug bulleted.app hit.
#[tokio::test]
async fn test_apply_writes_returns_commit_and_results() {
    let (_dir, client) = common::get_client().await;
    let token = get_access_token(&client).await;
    let did = "did:plc:khvyd3oiw46vif5gm7hijslk";
    client
        .rocket()
        .state::<rsky_pds::account_manager::AccountManager>()
        .unwrap()
        .activate_account(did)
        .await
        .unwrap();

    let response = client
        .post("/xrpc/com.atproto.repo.applyWrites")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .body(
            json!({
                "repo": did,
                "validate": false,
                "writes": [{
                    "$type": "com.atproto.repo.applyWrites#create",
                    "collection": "com.example.record",
                    "rkey": "aw1",
                    "value": {"$type": "com.example.record", "text": "batch"}
                }]
            })
            .to_string(),
        )
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: serde_json::Value = response.into_json().await.expect("a JSON body, not empty");
    assert!(body["commit"]["cid"].is_string(), "{body}");
    assert!(body["commit"]["rev"].is_string(), "{body}");
    let results = body["results"].as_array().expect("results array");
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0]["$type"],
        "com.atproto.repo.applyWrites#createResult"
    );
    assert!(results[0]["uri"]
        .as_str()
        .unwrap()
        .ends_with("/com.example.record/aw1"));
    assert!(results[0]["cid"].is_string());
}
