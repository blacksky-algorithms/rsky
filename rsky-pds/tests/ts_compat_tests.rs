//! Compatibility of rsky-pds with data written by the reference PDS.
//!
//! Every test runs against `tests/fixtures/ts-pds-0.5.27`, a data directory
//! produced by the pinned reference image, and compares rsky-pds responses
//! with the responses the reference PDS gave for the same requests.

mod common;

use common::{get_client_with_fixture, Fixture};
use rocket::http::Status;
use rocket::local::asynchronous::Client;
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
