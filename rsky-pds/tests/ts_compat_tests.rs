//! Compatibility of rsky-pds with data written by the reference PDS.
//!
//! Every test runs against `tests/fixtures/ts-pds-0.5.27`, a data directory
//! produced by the pinned reference image, and compares rsky-pds responses
//! with the responses the reference PDS gave for the same requests.

mod common;

use common::{get_admin_token, get_client_with_fixture, get_client_with_fixture_copy, Fixture};
use rocket::http::{ContentType, Header, Status};
use rocket::local::asynchronous::Client;
use rsky_oauth::jwk::Jwk;
use rsky_oauth::jwt::{JwtClaims, JwtHeader};
use rsky_pds::account_manager::helpers::auth::{
    create_access_token_with, create_refresh_token_with, CreateTokensOpts, JwtSigner,
};
use rsky_pds::auth_verifier::AuthScope;
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

fn dpop_key(session: &Value) -> Jwk {
    serde_json::from_value(session["dpop"]["private_jwk"].clone()).unwrap()
}

fn dpop_proof(
    key: &Jwk,
    method: &str,
    htu: &str,
    nonce: Option<&str>,
    access_token: Option<&str>,
) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use sha2::{Digest, Sha256};
    let mut header = JwtHeader::new("ES256");
    header.typ = Some("dpop+jwt".to_owned());
    header.jwk = Some(key.to_public());
    let mut claims = JwtClaims {
        iat: Some(rsky_pds::account_manager::helpers::auth::now_secs()),
        jti: Some(format!("jti-{}", rsky_common::get_random_str())),
        ..Default::default()
    };
    claims
        .extra
        .insert("htm".to_owned(), Value::String(method.to_owned()));
    claims
        .extra
        .insert("htu".to_owned(), Value::String(htu.to_owned()));
    if let Some(nonce) = nonce {
        claims
            .extra
            .insert("nonce".to_owned(), Value::String(nonce.to_owned()));
    }
    if let Some(token) = access_token {
        let ath = URL_SAFE_NO_PAD.encode(Sha256::digest(token.as_bytes()));
        claims.extra.insert("ath".to_owned(), Value::String(ath));
    }
    rsky_oauth::jwt::sign(&header, &claims, key).unwrap()
}

/// Sends a DPoP-bound request, retrying once with the server's nonce the
/// way a client does.
async fn dpop_request(
    client: &Client,
    method: &str,
    path: &str,
    key: &Jwk,
    access_token: Option<&str>,
    form: Option<&str>,
) -> (Status, Value, Option<String>) {
    let body = form.map(|form| (ContentType::Form, form.to_owned()));
    dpop_request_with(client, method, path, key, access_token, body).await
}

async fn dpop_request_with(
    client: &Client,
    method: &str,
    path: &str,
    key: &Jwk,
    access_token: Option<&str>,
    body: Option<(ContentType, String)>,
) -> (Status, Value, Option<String>) {
    let htu = format!("https://fixture.test{}", path.split('?').next().unwrap());
    let mut nonce: Option<String> = None;
    for _ in 0..2 {
        let proof = dpop_proof(key, method, &htu, nonce.as_deref(), access_token);
        let mut request = match method {
            "GET" => client.get(path),
            _ => client.post(path),
        };
        request = request.header(Header::new("DPoP", proof));
        if let Some(token) = access_token {
            request = request.header(Header::new("Authorization", format!("DPoP {token}")));
        }
        if let Some((content_type, body)) = body.clone() {
            request = request.header(content_type).body(body);
        }
        let response = request.dispatch().await;
        let status = response.status();
        let next_nonce = response.headers().get_one("DPoP-Nonce").map(str::to_owned);
        let www = response
            .headers()
            .get_one("WWW-Authenticate")
            .map(str::to_owned);
        let body = response.into_string().await.unwrap_or_default();
        let json: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
        let wants_nonce = json["error"] == "use_dpop_nonce";
        if wants_nonce && nonce.is_none() && next_nonce.is_some() {
            nonce = next_nonce;
            continue;
        }
        return (status, json, www);
    }
    unreachable!("nonce retry loop")
}

/// OAuth sessions the reference PDS issued (DPoP-bound HS256 tokens backed
/// by its token rows) authenticate on rsky-pds with the reference outputs.
#[tokio::test]
async fn reference_oauth_sessions_authenticate() {
    let (fixture, client) = get_client_with_fixture().await;
    let session = fixture.oauth("alice-fresh");
    let key = dpop_key(&session);
    let access = session["token"]["access_token"].as_str().unwrap();
    let path = "/xrpc/com.atproto.server.getSession";
    let (status, json, _) = dpop_request(&client, "GET", path, &key, Some(access), None).await;
    let exercised = fixture.oauth("alice-exercised");
    assert_eq!(
        status.code,
        exercised["session"]["status"].as_u64().unwrap() as u16,
        "{json}"
    );
    assert_eq!(json, exercised["session"]["body"]);

    // a token superseded by a refresh is refused the way the reference refuses it
    let refreshed = fixture.oauth("alice-refreshed");
    let stale = refreshed["token"]["access_token"].as_str().unwrap();
    let (status, json, www) = dpop_request(
        &client,
        "GET",
        path,
        &dpop_key(&refreshed),
        Some(stale),
        None,
    )
    .await;
    assert_eq!(
        status.code,
        exercised["session_after_refresh"]["status"]
            .as_u64()
            .unwrap() as u16
    );
    assert_eq!(json, exercised["session_after_refresh"]["body"]);
    assert!(www.unwrap_or_default().contains("error=\"invalid_token\""));
    let current = refreshed["refreshed"]["body"]["access_token"]
        .as_str()
        .unwrap();
    let (status, json, _) = dpop_request(
        &client,
        "GET",
        path,
        &dpop_key(&refreshed),
        Some(current),
        None,
    )
    .await;
    assert_eq!(status, Status::Ok, "{json}");

    // a method the reference PDS keeps away from OAuth sessions altogether
    let (status, json, _) = dpop_request_with(
        &client,
        "POST",
        "/xrpc/com.atproto.server.createAppPassword",
        &key,
        Some(access),
        Some((
            ContentType::JSON,
            serde_json::json!({"name": "oauth-made"}).to_string(),
        )),
    )
    .await;
    assert_eq!(
        status.code,
        exercised["forbidden_method"]["status"].as_u64().unwrap() as u16,
        "{json}"
    );
    assert_eq!(json, exercised["forbidden_method"]["body"]);

    // a proof signed by another key than the token is bound to
    let (status, json, _) = dpop_request(
        &client,
        "GET",
        path,
        &dpop_key(&refreshed),
        Some(access),
        None,
    )
    .await;
    assert_eq!(status, Status::Unauthorized);
    assert_eq!(json["error"], "invalid_token");

    // the OAuth metadata documents match the reference
    let (status, json) = get_json(&client, "/oauth/jwks").await;
    assert_eq!(status, Status::Ok);
    assert_eq!(json, fixture.expected_json("oauth_jwks.json"));
    let (status, json) = get_json(&client, "/.well-known/oauth-protected-resource").await;
    assert_eq!(status, Status::Ok);
    assert_eq!(json, fixture.expected_json("oauth_protected_resource.json"));
}

/// A refresh token the reference PDS issued rotates on rsky-pds, and a
/// replay of the consumed refresh token revokes the session.
#[tokio::test]
async fn reference_oauth_refresh_tokens_rotate() {
    let (fixture, _dir, client) = get_client_with_fixture_copy().await;
    let session = fixture.oauth("alice-fresh");
    let key = dpop_key(&session);
    let client_id = session["client_id"].as_str().unwrap();
    let refresh_token = session["token"]["refresh_token"].as_str().unwrap();
    let form = format!(
        "grant_type=refresh_token&refresh_token={}&client_id={}",
        urlencoding::encode(refresh_token),
        urlencoding::encode(client_id)
    );
    let (status, json, _) =
        dpop_request(&client, "POST", "/oauth/token", &key, None, Some(&form)).await;
    assert_eq!(status, Status::Ok, "{json}");
    assert_eq!(json["token_type"], "DPoP");
    assert_eq!(json["sub"], fixture.did("alice"));
    assert_eq!(json["scope"], session["token"]["scope"]);
    let access = json["access_token"].as_str().unwrap();
    assert_eq!(
        jwt_part(access, 0),
        serde_json::json!({"typ": "at+jwt", "alg": "HS256"})
    );
    let claims = jwt_part(access, 1);
    assert_eq!(claims["iss"], "https://fixture.test");
    assert_eq!(claims["aud"], "did:web:fixture.test");
    assert_eq!(claims["cnf"]["jkt"], session["dpop"]["jkt"]);
    assert!(claims.get("scope").is_none());
    let path = "/xrpc/com.atproto.server.getSession";
    let (status, body, _) = dpop_request(&client, "GET", path, &key, Some(access), None).await;
    assert_eq!(status, Status::Ok, "{body}");
    assert_eq!(body, fixture.oauth("alice-exercised")["session"]["body"]);

    // the reference-issued access token is superseded
    let stale = session["token"]["access_token"].as_str().unwrap();
    let (status, body, _) = dpop_request(&client, "GET", path, &key, Some(stale), None).await;
    assert_eq!(status, Status::Unauthorized);
    assert_eq!(
        body,
        serde_json::json!({"error": "invalid_token", "message": "Invalid token"})
    );

    // replaying the consumed refresh token is refused and revokes the session
    let (status, json, _) =
        dpop_request(&client, "POST", "/oauth/token", &key, None, Some(&form)).await;
    let exercised = fixture.oauth("alice-exercised");
    assert_eq!(
        status.code,
        exercised["refresh_replayed"]["status"].as_u64().unwrap() as u16,
        "{json}"
    );
    assert_eq!(json, exercised["refresh_replayed"]["body"]);
    let (status, _, _) = dpop_request(&client, "GET", path, &key, Some(access), None).await;
    assert_eq!(status, Status::Unauthorized);
}

fn fixture_signer(fixture: &Fixture) -> JwtSigner {
    JwtSigner::hmac(fixture.secret("PDS_JWT_SECRET").as_bytes())
}

fn access_token_for(fixture: &Fixture, did: &str, scope: AuthScope) -> String {
    create_access_token_with(
        &fixture_signer(fixture),
        CreateTokensOpts {
            did: did.to_owned(),
            service_did: "did:web:fixture.test".to_owned(),
            scope: Some(scope),
            jti: None,
            expires_in_secs: None,
            issued_at: None,
        },
    )
    .unwrap()
}

async fn post_empty_with(client: &Client, path: &str, token: &str) -> (Status, Value) {
    post_json_with(client, path, Some(token), None).await
}

fn insert_email_token(
    dir: &std::path::Path,
    purpose: &str,
    did: &str,
    token: &str,
    requested_at: &str,
) {
    let conn = rusqlite::Connection::open(dir.join("data").join("account.sqlite")).unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO email_token (purpose, did, token, \"requestedAt\") VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![purpose, did, token, requested_at],
    )
    .unwrap();
}

fn decode_event(bytes: &[u8]) -> Value {
    serde_ipld_dagcbor::from_slice(bytes).unwrap()
}

/// Deactivating and reactivating an account produces the same sequencer
/// events, byte for byte, as the reference PDS produced for the same
/// account.
#[tokio::test]
async fn account_status_transitions_match_the_reference() {
    let (fixture, dir, client) = get_client_with_fixture_copy().await;
    let frank = fixture.did("frank");
    let token = access_token_for(fixture, &frank, AuthScope::Access);
    let reference = fixture.reference_events(&frank);
    let before = common::sequencer_events(&dir.path().join("data/sequencer.sqlite"), &frank).len();
    let new_events = |count: usize| {
        let rows = common::sequencer_events(&dir.path().join("data/sequencer.sqlite"), &frank);
        assert_eq!(rows.len(), before + count, "new events");
        rows[before..].to_vec()
    };
    // the reference history: creation, two deactivations, two activations
    let deactivated: Vec<_> = reference
        .iter()
        .filter(|(_, kind, event)| {
            kind == "account" && decode_event(event)["status"] == "deactivated"
        })
        .collect();
    assert_eq!(deactivated.len(), 2);
    let first_activation = reference
        .iter()
        .position(|(seq, kind, event)| {
            *seq > deactivated[0].0 && kind == "account" && decode_event(event)["active"] == true
        })
        .unwrap();
    let activation_batch: Vec<_> = reference[first_activation..first_activation + 3].to_vec();

    let deactivate = "/xrpc/com.atproto.server.deactivateAccount";
    let (status, json) = post_json_with(
        &client,
        deactivate,
        Some(&token),
        Some(serde_json::json!({})),
    )
    .await;
    assert_eq!(
        status.code,
        fixture.expected_status("deactivateAccount_frank.json"),
        "{json}"
    );
    let events = new_events(1);
    assert_eq!(events[0].1, "account");
    assert_eq!(events[0].2, deactivated[0].2, "deactivation event bytes");
    assert_auth_json_matches(
        &client,
        fixture,
        "getSession_frank_deactivated.json",
        "/xrpc/com.atproto.server.getSession",
        &token,
    )
    .await;
    let (status, _) = post_json_with(
        &client,
        deactivate,
        Some(&token),
        Some(serde_json::json!({})),
    )
    .await;
    assert_eq!(
        status.code,
        fixture.expected_status("deactivateAccount_frank_again.json")
    );
    new_events(2);

    let activate = "/xrpc/com.atproto.server.activateAccount";
    let (status, json) = post_empty_with(&client, activate, &token).await;
    assert_eq!(
        status.code,
        fixture.expected_status("activateAccount_frank.json"),
        "{json}"
    );
    let events = new_events(5);
    for (ours, theirs) in events[2..].iter().zip(activation_batch.iter()) {
        assert_eq!(ours.1, theirs.1, "event type");
        assert_eq!(ours.2, theirs.2, "{} event bytes", theirs.1);
    }
    let (status, _) = post_empty_with(&client, activate, &token).await;
    assert_eq!(
        status.code,
        fixture.expected_status("activateAccount_frank_again.json")
    );
    new_events(8);

    // scope and status rules
    let app_password = fixture.token("alice_app_password_access");
    let (status, json) = post_json_with(
        &client,
        deactivate,
        Some(&app_password),
        Some(serde_json::json!({})),
    )
    .await;
    assert_eq!(
        status.code,
        fixture.expected_status("deactivateAccount_app_password.json")
    );
    assert_eq!(
        json,
        fixture.expected_json("deactivateAccount_app_password.json")
    );
    let carol = fixture.did("carol");
    let recovery = access_token_for(fixture, &carol, AuthScope::Takendown);
    let carol_before =
        common::sequencer_events(&dir.path().join("data/sequencer.sqlite"), &carol).len();
    let (status, json) = post_json_with(
        &client,
        deactivate,
        Some(&recovery),
        Some(serde_json::json!({})),
    )
    .await;
    assert_eq!(
        status.code,
        fixture.expected_status("deactivateAccount_carol.json"),
        "{json}"
    );
    let carol_events = common::sequencer_events(&dir.path().join("data/sequencer.sqlite"), &carol);
    assert_eq!(carol_events.len(), carol_before + 1);
    let carol_reference = fixture.reference_events(&carol);
    assert_eq!(
        carol_events.last().unwrap().2,
        carol_reference.last().unwrap().2
    );
    let (status, json) = post_empty_with(&client, activate, &recovery).await;
    assert_eq!(
        status.code,
        fixture.expected_status("activateAccount_carol.json")
    );
    assert_eq!(json, fixture.expected_json("activateAccount_carol.json"));
    let oauth = fixture.oauth("alice-fresh");
    let (status, json, _) = dpop_request_with(
        &client,
        "POST",
        deactivate,
        &dpop_key(&oauth),
        Some(oauth["token"]["access_token"].as_str().unwrap()),
        Some((ContentType::JSON, "{}".to_owned())),
    )
    .await;
    assert_eq!(status, Status::Forbidden, "{json}");
}

/// Email-token flows answer like the reference and change the same state.
#[tokio::test]
async fn email_token_flows_match_the_reference() {
    let (fixture, dir, client) = get_client_with_fixture_copy().await;
    let frank = fixture.did("frank");
    let now = rsky_common::now();
    insert_email_token(dir.path(), "reset_password", &frank, "FIXTURE-RESET", &now);
    insert_email_token(dir.path(), "confirm_email", &frank, "FIXTURE-CONFIRM", &now);
    insert_email_token(
        dir.path(),
        "update_email",
        &frank,
        "FIXTURE-OLD",
        "2020-01-01T00:00:00.000Z",
    );

    let reset = "/xrpc/com.atproto.server.resetPassword";
    for (name, body) in [
        (
            "resetPassword_bad_token.json",
            serde_json::json!({"token": "NOPE", "password": "new-password-123"}),
        ),
        (
            "resetPassword_long.json",
            serde_json::json!({"token": "FIXTURE-RESET", "password": "x".repeat(257)}),
        ),
        (
            "resetPassword_frank.json",
            serde_json::json!({"token": "fixture-reset", "password": "new-password-123"}),
        ),
    ] {
        let (status, json) = post_json_with(&client, reset, None, Some(body)).await;
        assert_eq!(status.code, fixture.expected_status(name), "{name}: {json}");
        if fixture.expected(name).is_empty() {
            assert_eq!(json, Value::Null, "{name}");
        } else {
            assert_eq!(json, fixture.expected_json(name), "{name}");
        }
    }
    let create = "/xrpc/com.atproto.server.createSession";
    let (status, json) = post_json_with(
        &client,
        create,
        None,
        Some(
            serde_json::json!({"identifier": "frank.fixture.test", "password": "new-password-123"}),
        ),
    )
    .await;
    assert_eq!(
        status.code,
        fixture.expected_status("createSession_frank_new_password.json"),
        "{json}"
    );
    assert_eq!(
        without_tokens(json.clone()),
        without_tokens(fixture.expected_json("createSession_frank_new_password.json"))
    );
    let frank_token = json["accessJwt"].as_str().unwrap().to_owned();
    let (status, json) = post_json_with(&client, create, None, Some(serde_json::json!({"identifier": "frank.fixture.test", "password": fixture.account("alice")["password"]}))).await;
    assert_eq!(
        status.code,
        fixture.expected_status("createSession_frank_old_password.json")
    );
    assert_eq!(
        json,
        fixture.expected_json("createSession_frank_old_password.json")
    );

    let confirm = "/xrpc/com.atproto.server.confirmEmail";
    for (name, body) in [
        (
            "confirmEmail_wrong_email.json",
            serde_json::json!({"email": "someone@fixture.invalid", "token": "FIXTURE-CONFIRM"}),
        ),
        (
            "confirmEmail_bad_token.json",
            serde_json::json!({"email": "frank@fixture.invalid", "token": "NOPE"}),
        ),
        (
            "confirmEmail_frank.json",
            serde_json::json!({"email": "FRANK@fixture.invalid", "token": "fixture-confirm"}),
        ),
    ] {
        let (status, json) = post_json_with(&client, confirm, Some(&frank_token), Some(body)).await;
        assert_eq!(status.code, fixture.expected_status(name), "{name}: {json}");
        if fixture.expected(name).is_empty() {
            assert_eq!(json, Value::Null, "{name}");
        } else {
            assert_eq!(json, fixture.expected_json(name), "{name}");
        }
    }
    assert_auth_json_matches(
        &client,
        fixture,
        "getSession_frank_confirmed.json",
        "/xrpc/com.atproto.server.getSession",
        &frank_token,
    )
    .await;
    let (status, json) = post_json_with(
        &client,
        "/xrpc/com.atproto.server.updateEmail",
        Some(&frank_token),
        Some(serde_json::json!({"email": "frank2@fixture.invalid", "token": "FIXTURE-OLD"})),
    )
    .await;
    assert_eq!(
        status.code,
        fixture.expected_status("updateEmail_expired_token.json"),
        "{json}"
    );
    assert_eq!(
        json,
        fixture.expected_json("updateEmail_expired_token.json")
    );
    // an email another account already uses is refused with the reference message
    let (status, json) = post_json_with(
        &client,
        "/xrpc/com.atproto.server.updateEmail",
        Some(&fixture.token("alice_access")),
        Some(serde_json::json!({"email": "bob@fixture.invalid"})),
    )
    .await;
    assert_eq!(status, Status::BadRequest, "{json}");
    assert_eq!(
        json["message"],
        "This email address is already in use, please use a different email."
    );
    let (status, json) = post_empty_with(
        &client,
        "/xrpc/com.atproto.server.requestAccountDelete",
        &fixture.token("alice_app_password_access"),
    )
    .await;
    assert_eq!(
        status.code,
        fixture.expected_status("requestAccountDelete_app_password.json")
    );
    assert_eq!(
        json,
        fixture.expected_json("requestAccountDelete_app_password.json")
    );
}

/// Deleting an account leaves what the reference PDS leaves: only the
/// deletion event in the sequencer, no rows, no repository, no session.
#[tokio::test]
async fn account_deletion_matches_the_reference() {
    let (fixture, dir, client) = get_client_with_fixture_copy().await;
    let dave = fixture.did("dave");
    let erin = fixture.did("erin");
    let password = fixture.account("alice")["password"].as_str().unwrap();
    let dave_token = access_token_for(fixture, &dave, AuthScope::Access);
    insert_email_token(
        dir.path(),
        "delete_account",
        &dave,
        "FIXTURE-DELETE-DAVE",
        &rsky_common::now(),
    );

    let delete = "/xrpc/com.atproto.server.deleteAccount";
    let substitute = |value: Value| -> Value {
        serde_json::from_str(&value.to_string().replace(&erin, &dave)).unwrap()
    };
    for (name, body) in [
        (
            "deleteAccount_wrong_password.json",
            serde_json::json!({"did": dave, "password": "nope", "token": "FIXTURE-DELETE-DAVE"}),
        ),
        (
            "deleteAccount_bad_token.json",
            serde_json::json!({"did": dave, "password": password, "token": "NOPE"}),
        ),
        (
            "deleteAccount_unknown.json",
            serde_json::json!({"did": "did:plc:aaaaaaaaaaaaaaaaaaaaaaaa", "password": password, "token": "NOPE"}),
        ),
        (
            "deleteAccount_erin.json",
            serde_json::json!({"did": dave, "password": password, "token": "fixture-delete-dave"}),
        ),
        (
            "deleteAccount_erin_again.json",
            serde_json::json!({"did": dave, "password": password, "token": "fixture-delete-dave"}),
        ),
    ] {
        let (status, json) = post_json_with(&client, delete, None, Some(body)).await;
        assert_eq!(status.code, fixture.expected_status(name), "{name}: {json}");
        if fixture.expected(name).is_empty() {
            assert_eq!(json, Value::Null, "{name}");
        } else {
            assert_eq!(json, substitute(fixture.expected_json(name)), "{name}");
        }
    }

    let (status, json) =
        get_json_with(&client, "/xrpc/com.atproto.server.getSession", &dave_token).await;
    assert_eq!(
        status.code,
        fixture.expected_status("getSession_erin_deleted.json")
    );
    assert_eq!(
        json,
        substitute(fixture.expected_json("getSession_erin_deleted.json"))
    );
    let (status, json) = post_json_with(
        &client,
        "/xrpc/com.atproto.server.createSession",
        None,
        Some(serde_json::json!({"identifier": "dave.fixture.test", "password": password})),
    )
    .await;
    assert_eq!(
        status.code,
        fixture.expected_status("createSession_erin_deleted.json")
    );
    assert_eq!(
        json,
        fixture.expected_json("createSession_erin_deleted.json")
    );
    let (status, json) = get_json(
        &client,
        &format!("/xrpc/com.atproto.sync.getRepo?did={dave}"),
    )
    .await;
    assert_eq!(status.code, fixture.expected_status("erin_getRepo.json"));
    assert_eq!(json, substitute(fixture.expected_json("erin_getRepo.json")));

    // the reference keeps exactly one event for a deleted account
    let ours = common::sequencer_events(&dir.path().join("data/sequencer.sqlite"), &dave);
    let theirs = fixture.reference_events(&erin);
    assert_eq!(theirs.len(), 1);
    assert_eq!(ours.len(), 1);
    assert_eq!(ours[0].1, "account");
    assert_eq!(
        decode_event(&ours[0].2),
        substitute(decode_event(&theirs[0].2))
    );
    let shard = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(dave.as_bytes()));
    assert!(!dir
        .path()
        .join("data/actors")
        .join(&shard[..2])
        .join(&dave)
        .exists());

    // the deletion journal: complete, with a purge obligation because blob
    // storage is shared during coexistence
    let conn = rusqlite::Connection::open(dir.path().join("data/rsky/lifecycle.sqlite")).unwrap();
    let (deleted_at, seq): (Option<String>, Option<i64>) = conn
        .query_row(
            "SELECT \"logicallyDeletedAt\", \"accountSeq\" FROM tombstone WHERE did = ?1",
            [&dave],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert!(deleted_at.is_some());
    assert_eq!(seq, Some(ours[0].0));
    let prefixes: String = conn
        .query_row(
            "SELECT \"namespacePrefixes\" FROM purge_obligation WHERE did = ?1",
            [&dave],
            |row| row.get(0),
        )
        .unwrap();
    assert!(prefixes.contains(&format!("blocks/{dave}/")));
}

/// The publication frontier over the reference-created fixture: an account
/// the reference created and hosted alone is complete, its exposed maximum
/// follows the reads this server serves, and accounts whose history or
/// identity cannot be established fail closed.
#[tokio::test]
async fn publication_frontier_classifies_the_fixture_accounts() {
    // the import below writes, so this test takes its own fixture copy
    let (fixture, _dir, client) = get_client_with_fixture_copy().await;
    let alice = fixture.did("alice");
    let frontier = |did: String| {
        let client = &client;
        async move {
            let response = client
                .get(format!(
                    "/xrpc/community.blacksky.pds.getPublicationFrontier?did={did}"
                ))
                .header(Header::new("Authorization", get_admin_token()))
                .dispatch()
                .await;
            let status = response.status();
            let json: Value = response.into_json().await.unwrap_or(Value::Null);
            (status, json)
        }
    };
    let unauthenticated = client
        .get(format!(
            "/xrpc/community.blacksky.pds.getPublicationFrontier?did={alice}"
        ))
        .dispatch()
        .await;
    assert_ne!(unauthenticated.status(), Status::Ok);

    let (status, json) = frontier(alice.clone()).await;
    assert_eq!(status, Status::Ok, "{json}");
    assert_eq!(json["complete"], true, "{json}");
    assert_eq!(json["genesisKind"], "creation");
    assert_eq!(json["lifetime"], "hostedHereOnly");
    let (_, latest) = get_json(
        &client,
        &format!("/xrpc/com.atproto.sync.getLatestCommit?did={alice}"),
    )
    .await;
    assert_eq!(json["publicationMaxRev"], latest["rev"]);
    assert_eq!(json["currentCommitCid"], latest["cid"]);
    assert_eq!(json["signedCommitRev"], latest["rev"]);
    assert_eq!(json["repoRootRev"], latest["rev"]);
    // the read above exposed the current revision, as every sync read and
    // an accepted import do
    let (_, json) = frontier(alice.clone()).await;
    assert_eq!(json["exposedMaxRev"], latest["rev"]);
    let (status, _) = get_bytes(
        &client,
        &format!(
            "/xrpc/com.atproto.sync.getBlocks?did={alice}&cids={}",
            latest["cid"].as_str().unwrap()
        ),
    )
    .await;
    assert_eq!(status, Status::Ok);
    let (status, car) = get_bytes(
        &client,
        &format!("/xrpc/com.atproto.sync.getRepo?did={alice}"),
    )
    .await;
    assert_eq!(status, Status::Ok);
    let import = client
        .post("/xrpc/com.atproto.repo.importRepo")
        .header(ContentType::new("application", "vnd.ipld.car"))
        .header(Header::new("content-length", car.len().to_string()))
        .header(Header::new(
            "Authorization",
            format!("Bearer {}", fixture.token("alice_access")),
        ))
        .body(car)
        .dispatch()
        .await;
    let status = import.status();
    let body = import.into_string().await.unwrap_or_default();
    assert_eq!(status, Status::Ok, "{body}");

    // deleted on the reference: only the deletion event survives
    let (_, json) = frontier(fixture.did("erin")).await;
    assert_eq!(json["complete"], false);
    assert_eq!(json["genesisKind"], "unknown");
    // no identity history to audit
    let (_, json) = frontier(fixture.did("dave")).await;
    assert_eq!(json["complete"], false);
    assert_eq!(json["lifetime"], "unknown");
    assert_eq!(json["genesisKind"], "creation");
}

/// The convergence report over the reference-created fixture: an account
/// the reference wrote and published agrees with itself; an account the
/// reference deleted is reported as deleted but not journaled here.
#[tokio::test]
async fn convergence_report_over_the_fixture_accounts() {
    let (fixture, client) = get_client_with_fixture().await;
    let report = |did: String| {
        let client = &client;
        async move {
            let response = client
                .get(format!(
                    "/xrpc/community.blacksky.pds.getConvergence?did={did}"
                ))
                .header(Header::new("Authorization", get_admin_token()))
                .dispatch()
                .await;
            let status = response.status();
            let json: Value = response.into_json().await.unwrap_or(Value::Null);
            (status, json)
        }
    };
    let (status, json) = report(fixture.did("alice")).await;
    assert_eq!(status, Status::Ok, "{json}");
    assert_eq!(json["converged"], true, "{json}");
    assert_eq!(json["storeRoot"], json["accountRoot"]);
    assert_eq!(json["storeRoot"], json["lastPublished"]);
    assert_eq!(json["account"]["consistent"], true);
    let (_, json) = report(fixture.did("erin")).await;
    assert_eq!(json["converged"], false);
    assert_eq!(json["deleted"], true);
    assert!(json["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason.as_str().unwrap().contains("not journaled")));
    let unauthenticated = client
        .get(format!(
            "/xrpc/community.blacksky.pds.getConvergence?did={}",
            fixture.did("alice")
        ))
        .dispatch()
        .await;
    assert_ne!(unauthenticated.status(), Status::Ok);
}
