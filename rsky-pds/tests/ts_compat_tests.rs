//! Compatibility of rsky-pds with data written by the reference PDS.
//!
//! Every test runs against `tests/fixtures/ts-pds-0.5.27`, a data directory
//! produced by the pinned reference image, and compares rsky-pds responses
//! with the responses the reference PDS gave for the same requests.

mod common;

use common::{get_client_with_fixture, get_client_with_fixture_copy, Fixture};
use rocket::http::{ContentType, Header, Status};
use rocket::local::asynchronous::Client;
use rsky_pds::account_manager::helpers::auth::{
    create_refresh_token_with, CreateTokensOpts, JwtSigner,
};
use rsky_repo::car::read_car;
use rsky_repo::sync::consumer::verify_proofs;
use rsky_repo::types::RecordCidClaim;
use rusqlite::Connection;
use serde_json::Value;

async fn get_json(client: &Client, path: &str) -> (Status, Value) {
    let response = client.get(path).dispatch().await;
    let status = response.status();
    let body = response.into_string().await.unwrap_or_default();
    let json = serde_json::from_str(&body).unwrap_or_else(|_| panic!("non-json body: {body}"));
    (status, json)
}

async fn get_bytes(client: &Client, path: &str) -> (Status, Vec<u8>) {
    let response = client.get(path).dispatch().await;
    let status = response.status();
    (status, response.into_bytes().await.unwrap_or_default())
}

async fn assert_json_matches(client: &Client, fixture: &Fixture, name: &str, path: &str) {
    let (status, json) = get_json(client, path).await;
    assert_eq!(status.code, fixture.expected_status(name), "{name}: {json}");
    assert_eq!(json, fixture.expected_json(name), "{name}");
}

/// Schema-level snapshot of a SQLite file: every object definition plus the
/// migration ledger rows and whether an rsky-only ledger exists.
fn schema_snapshot(path: &std::path::Path) -> Vec<String> {
    let conn =
        Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut rows: Vec<String> = conn
        .prepare(
            "SELECT type || ':' || name || ':' || coalesce(sql, '') FROM sqlite_master ORDER BY 1",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let ledger: Vec<String> = conn
        .prepare("SELECT name FROM kysely_migration ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    rows.push(format!("ledger:{}", ledger.join(",")));
    rows
}

fn source_data(fixture: &Fixture, relative: &str) -> std::path::PathBuf {
    fixture.source.join("data").join(relative)
}

#[tokio::test]
async fn reading_the_fixture_leaves_its_schema_untouched() {
    let (fixture, client) = get_client_with_fixture().await;
    let alice = fixture.did("alice");
    let store = fixture
        .actor_store("alice")
        .strip_prefix(fixture.data(""))
        .unwrap()
        .to_path_buf();
    let files = [
        "account.sqlite",
        "sequencer.sqlite",
        "did_cache.sqlite",
        store.to_str().unwrap(),
    ];
    for path in [
        format!("/xrpc/com.atproto.repo.describeRepo?repo={alice}"),
        format!("/xrpc/com.atproto.repo.listRecords?repo={alice}&collection=app.bsky.feed.post"),
        format!("/xrpc/com.atproto.sync.getRepo?did={alice}"),
        format!("/xrpc/com.atproto.sync.getLatestCommit?did={alice}"),
        "/xrpc/com.atproto.sync.listRepos".to_owned(),
    ] {
        let response = client.get(&path).dispatch().await;
        assert_eq!(response.status(), Status::Ok, "{path}");
    }
    for file in files {
        assert_eq!(
            schema_snapshot(&fixture.data(file)),
            schema_snapshot(&source_data(fixture, file)),
            "{file} schema changed after reads"
        );
    }
}

#[tokio::test]
async fn describe_repo_matches_the_reference() {
    let (fixture, client) = get_client_with_fixture().await;
    let alice = fixture.did("alice");
    assert_json_matches(
        &client,
        fixture,
        "describeRepo.json",
        &format!("/xrpc/com.atproto.repo.describeRepo?repo={alice}"),
    )
    .await;
}

#[tokio::test]
async fn list_records_matches_the_reference() {
    let (fixture, client) = get_client_with_fixture().await;
    let alice = fixture.did("alice");
    let base = format!(
        "/xrpc/com.atproto.repo.listRecords?repo={alice}&collection=app.bsky.feed.post&limit=5"
    );
    assert_json_matches(&client, fixture, "listRecords.json", &base).await;
    let page1 = fixture.expected_json("listRecords.json");
    let cursor = page1["cursor"].as_str().unwrap();
    assert_json_matches(
        &client,
        fixture,
        "listRecords_page2.json",
        &format!("{base}&cursor={cursor}"),
    )
    .await;
    assert_json_matches(
        &client,
        fixture,
        "listRecords_reverse.json",
        &format!("{base}&reverse=true"),
    )
    .await;
}

#[tokio::test]
async fn list_records_rejects_an_oversized_limit_like_the_reference() {
    let (fixture, client) = get_client_with_fixture().await;
    let alice = fixture.did("alice");
    assert_json_matches(
        &client,
        fixture,
        "alice_listRecords_limit101.json",
        &format!(
            "/xrpc/com.atproto.repo.listRecords?repo={alice}&collection=app.bsky.feed.post&limit=101"
        ),
    )
    .await;
}

/// The fixture's fourth account has no DID document anywhere the server can
/// look; the reference PDS reports that as `InvalidRequest`.
#[tokio::test]
async fn describe_repo_reports_an_unresolvable_did_like_the_reference() {
    let (fixture, client) = get_client_with_fixture().await;
    let dave = fixture.did("dave");
    let (status, json) = get_json(
        &client,
        &format!("/xrpc/com.atproto.repo.describeRepo?repo={dave}"),
    )
    .await;
    let expected = fixture.expected_json("dave_describeRepo.json");
    assert_eq!(
        status.code,
        fixture.expected_status("dave_describeRepo.json")
    );
    assert_eq!(json["error"], expected["error"]);
    assert!(json["message"]
        .as_str()
        .unwrap()
        .starts_with("Could not resolve DID: "));
}

#[tokio::test]
async fn get_record_matches_the_reference() {
    let (fixture, client) = get_client_with_fixture().await;
    let alice = fixture.did("alice");
    assert_json_matches(
        &client,
        fixture,
        "getRecord_profile.json",
        &format!("/xrpc/com.atproto.repo.getRecord?repo={alice}&collection=app.bsky.actor.profile&rkey=self"),
    )
    .await;
}

#[tokio::test]
async fn sync_status_reads_match_the_reference() {
    let (fixture, client) = get_client_with_fixture().await;
    let alice = fixture.did("alice");
    assert_json_matches(
        &client,
        fixture,
        "getLatestCommit.json",
        &format!("/xrpc/com.atproto.sync.getLatestCommit?did={alice}"),
    )
    .await;
    assert_json_matches(
        &client,
        fixture,
        "getRepoStatus.json",
        &format!("/xrpc/com.atproto.sync.getRepoStatus?did={alice}"),
    )
    .await;
    assert_json_matches(
        &client,
        fixture,
        "listBlobs.json",
        &format!("/xrpc/com.atproto.sync.listBlobs?did={alice}"),
    )
    .await;
}

#[tokio::test]
async fn list_repos_matches_the_reference() {
    let (fixture, client) = get_client_with_fixture().await;
    let (status, json) = get_json(&client, "/xrpc/com.atproto.sync.listRepos").await;
    assert_eq!(status, Status::Ok);
    let mut expected = fixture.expected_json("listRepos.json");
    // the cursor embeds the wall-clock time of the last row; the repos are the contract
    expected.as_object_mut().unwrap().remove("cursor");
    let mut actual = json;
    actual.as_object_mut().unwrap().remove("cursor");
    assert_eq!(actual, expected);
}

async fn assert_car_equivalent(actual: Vec<u8>, expected: Vec<u8>) {
    let actual = read_car(actual).await.unwrap();
    let expected = read_car(expected).await.unwrap();
    assert_eq!(actual.roots, expected.roots, "car roots");
    assert_eq!(actual.blocks.map, expected.blocks.map, "car blocks");
}

#[tokio::test]
async fn get_repo_is_car_equivalent_to_the_reference() {
    let (fixture, client) = get_client_with_fixture().await;
    let alice = fixture.did("alice");
    let (status, car) = get_bytes(
        &client,
        &format!("/xrpc/com.atproto.sync.getRepo?did={alice}"),
    )
    .await;
    assert_eq!(status, Status::Ok);
    assert_car_equivalent(car, fixture.expected("getRepo.car")).await;
}

#[tokio::test]
async fn sync_get_record_is_car_equivalent_and_proof_valid() {
    let (fixture, client) = get_client_with_fixture().await;
    let alice = fixture.did("alice");
    let did_key = fixture.did_key("alice");
    let (status, present) = get_bytes(
        &client,
        &format!("/xrpc/com.atproto.sync.getRecord?did={alice}&collection=app.bsky.actor.profile&rkey=self"),
    )
    .await;
    assert_eq!(status, Status::Ok);
    assert_car_equivalent(
        present.clone(),
        fixture.expected("syncGetRecord_profile.car"),
    )
    .await;
    let expected_cid = fixture.expected_json("getRecord_profile.json")["cid"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let claims = vec![RecordCidClaim {
        collection: "app.bsky.actor.profile".to_owned(),
        rkey: "self".to_owned(),
        cid: Some(expected_cid),
    }];
    let proofs = verify_proofs(present, claims.clone(), &alice, &did_key)
        .await
        .unwrap();
    assert!(proofs.unverified.is_empty());
    assert_eq!(proofs.verified.len(), 1);

    let (status, missing) = get_bytes(
        &client,
        &format!("/xrpc/com.atproto.sync.getRecord?did={alice}&collection=app.bsky.feed.post&rkey=3lnope"),
    )
    .await;
    assert_eq!(
        status.code,
        fixture.expected_status("syncGetRecord_missing.car")
    );
    assert_car_equivalent(
        missing.clone(),
        fixture.expected("syncGetRecord_missing.car"),
    )
    .await;
    let absent = vec![RecordCidClaim {
        collection: "app.bsky.feed.post".to_owned(),
        rkey: "3lnope".to_owned(),
        cid: None,
    }];
    let proofs = verify_proofs(missing, absent, &alice, &did_key)
        .await
        .unwrap();
    assert!(proofs.unverified.is_empty());
    assert_eq!(proofs.verified.len(), 1);
}

#[tokio::test]
async fn get_blob_matches_the_reference() {
    let (fixture, client) = get_client_with_fixture().await;
    let alice = fixture.did("alice");
    let cid = fixture.account("alice")["blob_cid"].as_str().unwrap();
    let (status, bytes) = get_bytes(
        &client,
        &format!("/xrpc/com.atproto.sync.getBlob?did={alice}&cid={cid}"),
    )
    .await;
    assert_eq!(status, Status::Ok);
    assert_eq!(bytes, fixture.expected("getBlob.png"));
}

#[tokio::test]
async fn deactivated_and_takendown_repos_report_reference_errors() {
    let (fixture, client) = get_client_with_fixture().await;
    let bob = fixture.did("bob");
    let carol = fixture.did("carol");
    assert_json_matches(
        &client,
        fixture,
        "bob_getRepo.json",
        &format!("/xrpc/com.atproto.sync.getRepo?did={bob}"),
    )
    .await;
    assert_json_matches(
        &client,
        fixture,
        "carol_getRepo.json",
        &format!("/xrpc/com.atproto.sync.getRepo?did={carol}"),
    )
    .await;
    assert_json_matches(
        &client,
        fixture,
        "carol_listRecords.json",
        &format!("/xrpc/com.atproto.repo.listRecords?repo={carol}&collection=app.bsky.feed.post"),
    )
    .await;
    assert_json_matches(
        &client,
        fixture,
        "carol_getLatestCommit.json",
        &format!("/xrpc/com.atproto.sync.getLatestCommit?did={carol}"),
    )
    .await;
    assert_json_matches(
        &client,
        fixture,
        "bob_listBlobs.json",
        &format!("/xrpc/com.atproto.sync.listBlobs?did={bob}"),
    )
    .await;
    assert_json_matches(
        &client,
        fixture,
        "bob_listRecords.json",
        &format!("/xrpc/com.atproto.repo.listRecords?repo={bob}&collection=app.bsky.feed.post"),
    )
    .await;
    assert_json_matches(
        &client,
        fixture,
        "bob_describeRepo.json",
        &format!("/xrpc/com.atproto.repo.describeRepo?repo={bob}"),
    )
    .await;
    let missing = "did:plc:aaaaaaaaaaaaaaaaaaaaaaaa";
    assert_json_matches(
        &client,
        fixture,
        "missing_getRepo.json",
        &format!("/xrpc/com.atproto.sync.getRepo?did={missing}"),
    )
    .await;
    assert_json_matches(
        &client,
        fixture,
        "missing_describeRepo.json",
        &format!("/xrpc/com.atproto.repo.describeRepo?repo={missing}"),
    )
    .await;
    assert_json_matches(
        &client,
        fixture,
        "missing_getLatestCommit.json",
        &format!("/xrpc/com.atproto.sync.getLatestCommit?did={missing}"),
    )
    .await;
}

#[tokio::test]
async fn describe_server_matches_the_reference() {
    let (fixture, client) = get_client_with_fixture().await;
    assert_json_matches(
        &client,
        fixture,
        "describeServer.json",
        "/xrpc/com.atproto.server.describeServer",
    )
    .await;
}

async fn get_json_with(client: &Client, path: &str, token: &str) -> (Status, Value) {
    let response = client
        .get(path)
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .dispatch()
        .await;
    let status = response.status();
    let body = response.into_string().await.unwrap_or_default();
    let json = serde_json::from_str(&body).unwrap_or_else(|_| panic!("non-json body: {body}"));
    (status, json)
}

async fn post_json_with(
    client: &Client,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (Status, Value) {
    let mut request = client.post(path);
    if let Some(token) = token {
        request = request.header(Header::new("Authorization", format!("Bearer {token}")));
    }
    if let Some(body) = body {
        request = request.header(ContentType::JSON).body(body.to_string());
    }
    let response = request.dispatch().await;
    let status = response.status();
    let body = response.into_string().await.unwrap_or_default();
    let json = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&body).unwrap_or_else(|_| panic!("non-json body: {body}"))
    };
    (status, json)
}

async fn assert_auth_json_matches(
    client: &Client,
    fixture: &Fixture,
    name: &str,
    path: &str,
    token: &str,
) {
    let (status, json) = get_json_with(client, path, token).await;
    assert_eq!(status.code, fixture.expected_status(name), "{name}: {json}");
    assert_eq!(json, fixture.expected_json(name), "{name}");
}

/// Session tokens minted by the reference PDS authenticate on rsky-pds and
/// produce the same session outputs.
#[tokio::test]
async fn reference_session_tokens_authenticate() {
    let (fixture, client) = get_client_with_fixture().await;
    let session = "/xrpc/com.atproto.server.getSession";
    for (name, token) in [
        ("getSession_alice.json", "alice_access"),
        ("getSession_app_password.json", "alice_app_password_access"),
        (
            "getSession_app_password_privileged.json",
            "alice_app_password_privileged_access",
        ),
        ("getSession_bob.json", "bob_access"),
    ] {
        assert_auth_json_matches(&client, fixture, name, session, &fixture.token(token)).await;
    }
    let passwords = "/xrpc/com.atproto.server.listAppPasswords";
    assert_auth_json_matches(
        &client,
        fixture,
        "listAppPasswords_access.json",
        passwords,
        &fixture.token("alice_access"),
    )
    .await;
    assert_auth_json_matches(
        &client,
        fixture,
        "listAppPasswords_app_password.json",
        passwords,
        &fixture.token("alice_app_password_access"),
    )
    .await;
}

/// Every rejection carries the reference error name and message.
#[tokio::test]
async fn reference_token_rejections_match() {
    let (fixture, client) = get_client_with_fixture().await;
    let session = "/xrpc/com.atproto.server.getSession";
    for (name, token) in [
        ("getSession_expired.json", "alice_expired_access"),
        ("getSession_typ_jwt.json", "alice_access_typ_jwt"),
        ("getSession_with_refresh.json", "alice_refresh"),
    ] {
        assert_auth_json_matches(&client, fixture, name, session, &fixture.token(token)).await;
    }
    let refresh = "/xrpc/com.atproto.server.refreshSession";
    let (status, json) =
        post_json_with(&client, refresh, Some(&fixture.token("alice_access")), None).await;
    assert_eq!(
        status.code,
        fixture.expected_status("refreshSession_with_access.json")
    );
    assert_eq!(
        json,
        fixture.expected_json("refreshSession_with_access.json")
    );

    let signer = JwtSigner::hmac(fixture.secret("PDS_JWT_SECRET").as_bytes());
    let unknown = create_refresh_token_with(
        &signer,
        CreateTokensOpts {
            did: fixture.did("alice"),
            service_did: "did:web:fixture.test".to_owned(),
            scope: None,
            jti: Some("unknown-jti".to_owned()),
            expires_in_secs: None,
            issued_at: None,
        },
    )
    .unwrap();
    let (status, json) = post_json_with(&client, refresh, Some(&unknown), None).await;
    assert_eq!(
        status.code,
        fixture.expected_status("refreshSession_unknown_jti.json")
    );
    assert_eq!(
        json,
        fixture.expected_json("refreshSession_unknown_jti.json")
    );

    let response = client.get(session).dispatch().await;
    assert_eq!(response.status(), Status::Unauthorized);
    let json: Value = serde_json::from_str(&response.into_string().await.unwrap()).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"error": "AuthMissing", "message": "Authentication Required"})
    );

    // a session token whose scope the route does not accept
    assert_auth_json_matches(
        &client,
        fixture,
        "listAppPasswords_signup_queued.json",
        "/xrpc/com.atproto.server.listAppPasswords",
        &fixture.token("alice_signup_queued_access"),
    )
    .await;

    // reference-signed tokens with a malformed or missing subject
    let mint = |claims: Value| signer.sign("at+jwt", &claims).unwrap();
    let now = rsky_pds::account_manager::helpers::auth::now_secs();
    for token in [
        mint(
            serde_json::json!({"scope": "com.atproto.access", "aud": "did:web:fixture.test", "sub": "alice", "iat": now, "exp": now + 60}),
        ),
        mint(
            serde_json::json!({"scope": "com.atproto.access", "aud": "did:web:fixture.test", "iat": now, "exp": now + 60}),
        ),
    ] {
        let (status, json) = get_json_with(&client, session, &token).await;
        assert_eq!(status, Status::BadRequest);
        assert_eq!(
            json,
            serde_json::json!({"error": "InvalidToken", "message": "Malformed token"})
        );
    }

    // refresh tokens for accounts the server cannot serve
    let refresh_for = |did: &str| {
        create_refresh_token_with(
            &signer,
            CreateTokensOpts {
                did: did.to_owned(),
                service_did: "did:web:fixture.test".to_owned(),
                scope: None,
                jti: Some(format!("jti-{did}")),
                expires_in_secs: None,
                issued_at: None,
            },
        )
        .unwrap()
    };
    let missing = "did:plc:aaaaaaaaaaaaaaaaaaaaaaaa";
    let (status, json) = post_json_with(&client, refresh, Some(&refresh_for(missing)), None).await;
    assert_eq!(status, Status::BadRequest);
    assert_eq!(
        json,
        serde_json::json!({"error": "InvalidRequest", "message": format!("Could not find user info for account: {missing}")})
    );
    let carol = fixture.did("carol");
    let (status, json) = post_json_with(&client, refresh, Some(&refresh_for(&carol)), None).await;
    assert_eq!(status, Status::Unauthorized);
    assert_eq!(
        json,
        serde_json::json!({"error": "AccountTakedown", "message": "Account has been taken down"})
    );
}

fn without_tokens(mut value: Value) -> Value {
    let object = value.as_object_mut().unwrap();
    object.remove("accessJwt");
    object.remove("refreshJwt");
    value
}

fn jwt_part(token: &str, index: usize) -> Value {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let part = token.split('.').nth(index).unwrap();
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(part).unwrap()).unwrap()
}

/// A refresh token minted by the reference PDS rotates on rsky-pds with the
/// reference grace semantics, and the tokens rsky-pds mints back are
/// reference-shaped.
#[tokio::test]
async fn reference_refresh_tokens_rotate() {
    let (fixture, _dir, client) = get_client_with_fixture_copy().await;
    let refresh = "/xrpc/com.atproto.server.refreshSession";
    let old = fixture.token("alice_refresh_2");
    let (status, first) = post_json_with(&client, refresh, Some(&old), None).await;
    assert_eq!(status, Status::Ok, "{first}");
    assert_eq!(
        without_tokens(first.clone()),
        without_tokens(fixture.expected_json("refreshSession_alice.json"))
    );
    let access = first["accessJwt"].as_str().unwrap();
    let next = first["refreshJwt"].as_str().unwrap();
    assert_eq!(
        jwt_part(access, 0),
        serde_json::json!({"typ": "at+jwt", "alg": "HS256"})
    );
    assert_eq!(
        jwt_part(next, 0),
        serde_json::json!({"typ": "refresh+jwt", "alg": "HS256"})
    );
    assert_eq!(jwt_part(access, 1)["scope"], "com.atproto.access");
    assert_eq!(jwt_part(next, 1)["scope"], "com.atproto.refresh");
    assert_eq!(jwt_part(next, 1)["sub"], fixture.did("alice"));

    // the new access token works, and the old refresh token keeps yielding
    // the same successor during its grace period
    let (status, session) =
        get_json_with(&client, "/xrpc/com.atproto.server.getSession", access).await;
    assert_eq!(status, Status::Ok);
    assert_eq!(session, fixture.expected_json("getSession_alice.json"));
    let (status, again) = post_json_with(&client, refresh, Some(&old), None).await;
    assert_eq!(status, Status::Ok);
    assert_eq!(
        jwt_part(again["refreshJwt"].as_str().unwrap(), 1)["jti"],
        jwt_part(next, 1)["jti"]
    );

    // deleting the session with the successor revokes it
    let (status, _) = post_json_with(
        &client,
        "/xrpc/com.atproto.server.deleteSession",
        Some(next),
        None,
    )
    .await;
    assert_eq!(status, Status::Ok);
    let (status, revoked) = post_json_with(&client, refresh, Some(next), None).await;
    assert_eq!(
        status.code,
        fixture.expected_status("refreshSession_unknown_jti.json")
    );
    assert_eq!(
        revoked,
        fixture.expected_json("refreshSession_unknown_jti.json")
    );
}

/// Logging in with the reference PDS's stored password hashes gives the
/// reference outputs for every account state.
#[tokio::test]
async fn create_session_matches_the_reference() {
    let (fixture, _dir, client) = get_client_with_fixture_copy().await;
    let create = "/xrpc/com.atproto.server.createSession";
    let password = fixture.account("alice")["password"].as_str().unwrap();
    let app_password = fixture.account("alice")["app_password"]["password"]
        .as_str()
        .unwrap();
    let cases = [
        (
            "createSession_alice.json",
            serde_json::json!({"identifier": "alice.fixture.test", "password": password}),
        ),
        (
            "createSession_alice_email.json",
            serde_json::json!({"identifier": "ALICE@fixture.invalid", "password": password}),
        ),
        (
            "createSession_alice_app_password.json",
            serde_json::json!({"identifier": fixture.did("alice"), "password": app_password}),
        ),
        (
            "createSession_wrong_password.json",
            serde_json::json!({"identifier": "alice.fixture.test", "password": "nope"}),
        ),
        (
            "createSession_unknown.json",
            serde_json::json!({"identifier": "nobody.fixture.test", "password": password}),
        ),
        (
            "createSession_bob.json",
            serde_json::json!({"identifier": "bob.fixture.test", "password": password}),
        ),
        (
            "createSession_carol.json",
            serde_json::json!({"identifier": "carol.fixture.test", "password": password}),
        ),
        (
            "createSession_carol_wrong_password.json",
            serde_json::json!({"identifier": "carol.fixture.test", "password": "nope", "allowTakendown": true}),
        ),
        (
            "createSession_carol_allow_takendown.json",
            serde_json::json!({"identifier": "carol.fixture.test", "password": password, "allowTakendown": true}),
        ),
        (
            "createSession_long_password.json",
            serde_json::json!({"identifier": "alice.fixture.test", "password": "x".repeat(513)}),
        ),
    ];
    for (name, body) in cases {
        let (status, json) = post_json_with(&client, create, None, Some(body)).await;
        assert_eq!(status.code, fixture.expected_status(name), "{name}: {json}");
        assert_eq!(
            without_tokens(json.clone()),
            without_tokens(fixture.expected_json(name)),
            "{name}"
        );
        if let Some(access) = json["accessJwt"].as_str() {
            assert_eq!(
                jwt_part(access, 0),
                serde_json::json!({"typ": "at+jwt", "alg": "HS256"}),
                "{name}"
            );
            let expected_scope = match name {
                "createSession_alice_app_password.json" => "com.atproto.appPass",
                "createSession_carol_allow_takendown.json" => "com.atproto.takendown",
                _ => "com.atproto.access",
            };
            assert_eq!(jwt_part(access, 1)["scope"], expected_scope, "{name}");
        }
    }
}
