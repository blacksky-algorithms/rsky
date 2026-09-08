//! Account migration: `checkAccountStatus`, `importRepo`, `listMissingBlobs`,
//! the `sync` proof endpoints an importing server reads, the
//! activate/deactivate state machine, and the export -> import round trip
//! between two independent server instances.
//!
//! The final section holds tests that assert protocol behaviour rsky-pds does
//! not yet produce; each names what it expects and why it matters.

use lexicon_cid::Cid;
use rocket::http::{ContentType, Header, Status};
use rocket::local::asynchronous::{Client, LocalResponse};
use rsky_lexicon::com::atproto::server::CreateInviteCodeOutput;
use rsky_pds::config::ServerConfig;
use serde_json::{json, Value};
use tokio::sync::{Mutex, MutexGuard};

mod common;

use crate::common::{get_admin_token, set_published_signing_key};

const DID: &str = "did:plc:khvyd3oiw46vif5gm7hijslk";
const UNKNOWN_DID: &str = "did:plc:zzzzzzzzzzzzzzzzzzzzzzzz";
/// Well-formed, and belongs to no repo any test here builds.
const ABSENT_CID: &str = "bafyreihv5qx4d7qnvqsrn3nxb4xn77aedsj4irmkutvvq7cthm7z5oqxqy";

/// The mock PLC directory serves one signing key for every DID it answers
/// for, so an activation -- which asserts the published document names the
/// account's own key -- has to be the only one in flight.
static PLC_KEY: Mutex<()> = Mutex::const_new(());

fn car_type() -> ContentType {
    ContentType::new("application", "vnd.ipld.car")
}

fn bearer(token: &str) -> Header<'static> {
    Header::new("Authorization", format!("Bearer {token}"))
}

async fn json_body(response: LocalResponse<'_>) -> Value {
    response.into_json().await.expect("a JSON body")
}

/// Creates an account holding `DID` and returns its access token. Accounts
/// created with a caller-supplied did start deactivated, which is the state a
/// migration target is in when the CAR arrives.
async fn account(client: &Client) -> String {
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
                "did": DID,
                "email": "foo@example.com",
                "handle": format!("foo{domain}"),
                "password": "password",
                "inviteCode": invite,
            })
            .to_string(),
        )
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    json_body(response).await["accessJwt"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn published_key_lock() -> MutexGuard<'static, ()> {
    PLC_KEY.lock().await
}

/// Publishes this account's signing key to the mock directory and activates
/// it. The caller must hold [`published_key_lock`] for as long as the key has
/// to stay published.
async fn activate(client: &Client, token: &str) {
    let credentials = json_body(
        client
            .get("/xrpc/com.atproto.identity.getRecommendedDidCredentials")
            .header(bearer(token))
            .dispatch()
            .await,
    )
    .await;
    let key = credentials["verificationMethods"]["atproto"]
        .as_str()
        .unwrap()
        .to_string();
    set_published_signing_key(Some(key));

    let response = client
        .post("/xrpc/com.atproto.server.activateAccount")
        .header(bearer(token))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
}

async fn deactivate(client: &Client, token: &str) {
    let response = client
        .post("/xrpc/com.atproto.server.deactivateAccount")
        .header(ContentType::JSON)
        .header(bearer(token))
        .body("{}")
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
}

async fn check_account_status(client: &Client, token: &str) -> Value {
    let response = client
        .get("/xrpc/com.atproto.server.checkAccountStatus")
        .header(bearer(token))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    json_body(response).await
}

async fn create_record(client: &Client, token: &str, rkey: &str, record: Value) -> Status {
    client
        .post("/xrpc/com.atproto.repo.createRecord")
        .header(ContentType::JSON)
        .header(bearer(token))
        .body(
            json!({
                "repo": DID,
                "collection": "com.example.record",
                "rkey": rkey,
                "validate": false,
                "record": record,
            })
            .to_string(),
        )
        .dispatch()
        .await
        .status()
}

async fn list_records(client: &Client) -> Value {
    json_body(
        client
            .get(format!(
                "/xrpc/com.atproto.repo.listRecords?repo={DID}&collection=com.example.record"
            ))
            .dispatch()
            .await,
    )
    .await
}

async fn export_repo(client: &Client) -> Vec<u8> {
    let response = client
        .get(format!("/xrpc/com.atproto.sync.getRepo?did={DID}"))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    assert_eq!(
        response.headers().get_one("Content-Type"),
        Some("application/vnd.ipld.car")
    );
    response.into_bytes().await.unwrap()
}

/// `importRepo` streams its body against the declared `Content-Length`, which
/// the local client does not set on its own.
async fn import_repo<'c>(client: &'c Client, token: &str, car: &[u8]) -> LocalResponse<'c> {
    client
        .post("/xrpc/com.atproto.repo.importRepo")
        .header(car_type())
        .header(bearer(token))
        .header(Header::new("Content-Length", car.len().to_string()))
        .body(car)
        .dispatch()
        .await
}

async fn upload_blob(client: &Client, token: &str, bytes: &'static str) -> Value {
    json_body(
        client
            .post("/xrpc/com.atproto.repo.uploadBlob")
            .header(ContentType::Plain)
            .header(bearer(token))
            .body(bytes)
            .dispatch()
            .await,
    )
    .await["blob"]
        .clone()
}

/// Activates an account, gives it two records -- one of them referencing an
/// uploaded blob -- and returns the exported repo CAR.
async fn seed_and_export(client: &Client, token: &str) -> Vec<u8> {
    activate(client, token).await;
    let blob = upload_blob(client, token, "hello blob").await;
    assert_eq!(
        create_record(
            client,
            token,
            "one",
            json!({"$type": "com.example.record", "text": "one", "img": blob})
        )
        .await,
        Status::Ok
    );
    assert_eq!(
        create_record(
            client,
            token,
            "two",
            json!({"$type": "com.example.record", "text": "two"})
        )
        .await,
        Status::Ok
    );
    export_repo(client).await
}

/// Bytes an unsigned-varint length prefix occupies in a CAR frame.
fn varint_len(mut n: usize) -> usize {
    let mut len = 1;
    while n >= 0x80 {
        n >>= 7;
        len += 1;
    }
    len
}

/// Bytes one `cid || block` CAR frame occupies, prefix included.
fn framed_len(cid: &Cid, block: &[u8]) -> usize {
    let payload = cid.to_bytes().len() + block.len();
    varint_len(payload) + payload
}

// ---------------------------------------------------------------------------
// com.atproto.server.checkAccountStatus
// ---------------------------------------------------------------------------

/// The census a migration is driven from: the counts a client compares
/// between source and target to decide the move is complete.
#[tokio::test]
async fn check_account_status_reports_an_exact_repo_census() {
    let _plc = published_key_lock().await;
    let (_dir, client) = common::get_client().await;
    let token = account(&client).await;
    activate(&client, &token).await;

    let empty = check_account_status(&client, &token).await;
    assert_eq!(empty["activated"], true);
    assert_eq!(empty["validDid"], true);
    assert_eq!(empty["repoBlocks"], 2, "{empty}");
    assert_eq!(empty["indexedRecords"], 0);
    assert_eq!(empty["privateStateValues"], 0);
    assert_eq!(empty["expectedBlobs"], 0);
    assert_eq!(empty["importedBlobs"], 0);
    let commit = empty["repoCommit"].as_str().unwrap().to_string();
    assert_eq!(empty["repoRev"].as_str().unwrap().len(), 13, "{empty}");

    assert_eq!(
        create_record(
            &client,
            &token,
            "one",
            json!({"$type": "com.example.record", "text": "one"})
        )
        .await,
        Status::Ok
    );

    let after = check_account_status(&client, &token).await;
    assert_eq!(after["repoBlocks"], 3, "{after}");
    assert_eq!(after["indexedRecords"], 1);
    assert_ne!(after["repoCommit"].as_str().unwrap(), commit);
    assert_ne!(after["repoRev"], empty["repoRev"]);
}

/// A migrating account is deactivated on both ends for part of the move, and
/// the census is exactly what a client polls during it.
#[tokio::test]
async fn check_account_status_answers_while_the_account_is_deactivated() {
    let _plc = published_key_lock().await;
    let (_dir, client) = common::get_client().await;
    let token = account(&client).await;
    activate(&client, &token).await;
    assert_eq!(
        create_record(
            &client,
            &token,
            "one",
            json!({"$type": "com.example.record", "text": "one"})
        )
        .await,
        Status::Ok
    );
    deactivate(&client, &token).await;

    let status = check_account_status(&client, &token).await;
    assert_eq!(status["activated"], false);
    assert_eq!(status["repoBlocks"], 3, "{status}");
    assert_eq!(status["indexedRecords"], 1);
}

/// Blob counts split the way a migration needs them: the source holds every
/// blob its records reference, the target holds none until they are carried
/// over separately.
#[tokio::test]
async fn check_account_status_counts_expected_and_imported_blobs_separately() {
    let _plc = published_key_lock().await;
    let (_dir, source) = common::get_client().await;
    let source_token = account(&source).await;
    let car = seed_and_export(&source, &source_token).await;

    let source_status = check_account_status(&source, &source_token).await;
    assert_eq!(source_status["expectedBlobs"], 1);
    assert_eq!(source_status["importedBlobs"], 1);

    let (_dir2, target) = common::get_client().await;
    let target_token = account(&target).await;
    assert_eq!(
        import_repo(&target, &target_token, &car).await.status(),
        Status::Ok
    );

    let target_status = check_account_status(&target, &target_token).await;
    assert_eq!(target_status["expectedBlobs"], 1);
    assert_eq!(target_status["importedBlobs"], 0);
}

// ---------------------------------------------------------------------------
// com.atproto.repo.listMissingBlobs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_missing_blobs_is_empty_when_every_referenced_blob_is_present() {
    let _plc = published_key_lock().await;
    let (_dir, client) = common::get_client().await;
    let token = account(&client).await;
    seed_and_export(&client, &token).await;

    for query in ["", "?limit=10"] {
        let response = client
            .get(format!("/xrpc/com.atproto.repo.listMissingBlobs{query}"))
            .header(bearer(&token))
            .dispatch()
            .await;
        assert_eq!(response.status(), Status::Ok);
        let body = json_body(response).await;
        assert_eq!(body["blobs"].as_array().unwrap().len(), 0, "{body}");
        assert!(body.get("cursor").is_none(), "{body}");
    }
}

/// `limit` bounds the page: a second referenced-but-absent blob must not
/// arrive in a page of one.
#[tokio::test]
async fn list_missing_blobs_respects_the_limit() {
    let _plc = published_key_lock().await;
    let (_dir, source) = common::get_client().await;
    let source_token = account(&source).await;
    activate(&source, &source_token).await;

    for (rkey, bytes) in [("one", "first blob"), ("two", "second blob")] {
        let blob = upload_blob(&source, &source_token, bytes).await;
        assert_eq!(
            create_record(
                &source,
                &source_token,
                rkey,
                json!({"$type": "com.example.record", "img": blob})
            )
            .await,
            Status::Ok
        );
    }
    let car = export_repo(&source).await;

    let (_dir2, target) = common::get_client().await;
    let target_token = account(&target).await;
    assert_eq!(
        import_repo(&target, &target_token, &car).await.status(),
        Status::Ok
    );

    let page = json_body(
        target
            .get("/xrpc/com.atproto.repo.listMissingBlobs?limit=1")
            .header(bearer(&target_token))
            .dispatch()
            .await,
    )
    .await;
    assert_eq!(page["blobs"].as_array().unwrap().len(), 1, "{page}");

    let all = json_body(
        target
            .get("/xrpc/com.atproto.repo.listMissingBlobs")
            .header(bearer(&target_token))
            .dispatch()
            .await,
    )
    .await;
    assert_eq!(all["blobs"].as_array().unwrap().len(), 2, "{all}");
}

// ---------------------------------------------------------------------------
// com.atproto.repo.importRepo
// ---------------------------------------------------------------------------

#[tokio::test]
async fn import_repo_rejects_an_empty_body() {
    let (_dir, client) = common::get_client().await;
    let token = account(&client).await;

    let response = import_repo(&client, &token, &[]).await;
    assert_eq!(response.status(), Status::BadRequest);
    assert_eq!(json_body(response).await["error"], "InvalidRequest");
}

/// The lexicon's input encoding is `application/vnd.ipld.car`; a JSON body
/// is not a repository and must be refused rather than half-imported.
#[tokio::test]
async fn import_repo_rejects_a_body_that_is_not_a_car() {
    let (_dir, client) = common::get_client().await;
    let token = account(&client).await;

    let response = client
        .post("/xrpc/com.atproto.repo.importRepo")
        .header(ContentType::JSON)
        .header(bearer(&token))
        .header(Header::new("Content-Length", "2"))
        .body("{}")
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::BadRequest);
    assert_eq!(json_body(response).await["error"], "InvalidRequest");
}

/// The size ceiling is enforced from the declared `Content-Length`, before a
/// byte of the body is read.
#[tokio::test]
async fn import_repo_rejects_a_car_over_the_import_limit() {
    let (_dir, client) = common::get_client().await;
    let token = account(&client).await;

    let response = client
        .post("/xrpc/com.atproto.repo.importRepo")
        .header(car_type())
        .header(bearer(&token))
        .header(Header::new("Content-Length", "10000000000"))
        .body(vec![0u8; 8])
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::BadRequest);
    let body = json_body(response).await;
    assert_eq!(body["error"], "InvalidRequest");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .starts_with("Content-Length is greater than maximum of"),
        "{body}"
    );
}

/// A CAR that dies mid-stream must leave the target exactly as it was. A
/// half-applied import is the intermediate state a migration cannot recover
/// from on its own: the account looks migrated and is missing records.
#[tokio::test]
async fn import_repo_leaves_the_target_untouched_when_the_car_is_truncated() {
    let _plc = published_key_lock().await;
    let (_dir, source) = common::get_client().await;
    let source_token = account(&source).await;
    let car = seed_and_export(&source, &source_token).await;

    let (_dir2, target) = common::get_client().await;
    let target_token = account(&target).await;
    let before = check_account_status(&target, &target_token).await;

    assert_ne!(
        import_repo(&target, &target_token, &car[..car.len() / 2])
            .await
            .status(),
        Status::Ok
    );

    let after = check_account_status(&target, &target_token).await;
    assert_eq!(after["repoCommit"], before["repoCommit"]);
    assert_eq!(after["repoBlocks"], before["repoBlocks"]);
    assert_eq!(after["indexedRecords"], 0);

    activate(&target, &target_token).await;
    assert_eq!(list_records(&target).await["records"], json!([]));
}

/// Migration steps get retried. Replaying the same CAR must be a no-op, not
/// a second set of records.
#[tokio::test]
async fn importing_the_same_car_twice_changes_nothing() {
    let _plc = published_key_lock().await;
    let (_dir, source) = common::get_client().await;
    let source_token = account(&source).await;
    let car = seed_and_export(&source, &source_token).await;

    let (_dir2, target) = common::get_client().await;
    let target_token = account(&target).await;

    assert_eq!(
        import_repo(&target, &target_token, &car).await.status(),
        Status::Ok
    );
    let after_first = check_account_status(&target, &target_token).await;

    assert_eq!(
        import_repo(&target, &target_token, &car).await.status(),
        Status::Ok
    );
    assert_eq!(
        check_account_status(&target, &target_token).await,
        after_first
    );
}

// ---------------------------------------------------------------------------
// com.atproto.sync.getBlocks / com.atproto.sync.getRecord
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sync_get_blocks_returns_a_car_of_exactly_the_requested_blocks() {
    let _plc = published_key_lock().await;
    let (_dir, client) = common::get_client().await;
    let token = account(&client).await;
    activate(&client, &token).await;
    assert_eq!(
        create_record(
            &client,
            &token,
            "one",
            json!({"$type": "com.example.record", "text": "one"})
        )
        .await,
        Status::Ok
    );

    let export = rsky_repo::car::read_car(export_repo(&client).await)
        .await
        .expect("a parseable repo CAR");
    let head = export.roots[0];
    let head_block = export.blocks.get(head).expect("the commit block").clone();
    let rest: Vec<Cid> = export
        .blocks
        .cids()
        .unwrap()
        .into_iter()
        .filter(|cid| *cid != head)
        .collect();
    assert_eq!(rest.len(), 2, "commit, mst root and one record");

    let one = client
        .get(format!(
            "/xrpc/com.atproto.sync.getBlocks?did={DID}&cids={head}"
        ))
        .dispatch()
        .await;
    assert_eq!(one.status(), Status::Ok);
    assert_eq!(
        one.headers().get_one("Content-Type"),
        Some("application/vnd.ipld.car")
    );
    let one = one.into_bytes().await.unwrap();
    let frame = [head.to_bytes(), head_block].concat();
    assert!(one.ends_with(&frame), "the block is carried verbatim");

    let cids = rest
        .iter()
        .map(|cid| format!("&cids={cid}"))
        .collect::<String>();
    let all = client
        .get(format!(
            "/xrpc/com.atproto.sync.getBlocks?did={DID}&cids={head}{cids}"
        ))
        .dispatch()
        .await;
    assert_eq!(all.status(), Status::Ok);
    let all = all.into_bytes().await.unwrap();
    let extra: usize = rest
        .iter()
        .map(|cid| framed_len(cid, export.blocks.get(*cid).unwrap()))
        .sum();
    assert_eq!(
        all.len(),
        one.len() + extra,
        "the one-cid CAR carried one block and nothing else"
    );
}

/// A request missing a required parameter is a client error, not a server
/// fault.
#[tokio::test]
async fn sync_endpoints_reject_a_request_missing_a_required_parameter() {
    let (_dir, client) = common::get_client().await;

    for path in [
        "/xrpc/com.atproto.sync.getBlocks".to_string(),
        "/xrpc/com.atproto.sync.getRecord".to_string(),
        format!("/xrpc/com.atproto.sync.getRecord?did={DID}"),
        format!("/xrpc/com.atproto.sync.getRecord?did={DID}&collection=com.example.record"),
    ] {
        let response = client.get(path.clone()).dispatch().await;
        assert_eq!(response.status(), Status::BadRequest, "{path}");
        assert_eq!(
            json_body(response).await["error"],
            "InvalidRequest",
            "{path}"
        );
    }
}

// ---------------------------------------------------------------------------
// activateAccount / deactivateAccount
// ---------------------------------------------------------------------------

/// The state machine both halves of a migration walk, and it has to survive
/// being walked twice: every step gets retried.
#[tokio::test]
async fn activate_and_deactivate_round_trip_and_are_idempotent() {
    let _plc = published_key_lock().await;
    let (_dir, client) = common::get_client().await;
    let token = account(&client).await;

    assert_eq!(
        check_account_status(&client, &token).await["activated"],
        false
    );

    activate(&client, &token).await;
    activate(&client, &token).await;
    assert_eq!(
        check_account_status(&client, &token).await["activated"],
        true
    );
    assert_eq!(
        create_record(
            &client,
            &token,
            "one",
            json!({"$type": "com.example.record", "text": "one"})
        )
        .await,
        Status::Ok
    );

    deactivate(&client, &token).await;
    deactivate(&client, &token).await;
    assert_eq!(
        check_account_status(&client, &token).await["activated"],
        false
    );
    assert_ne!(
        create_record(
            &client,
            &token,
            "blocked",
            json!({"$type": "com.example.record", "text": "blocked"})
        )
        .await,
        Status::Ok,
        "a deactivated account must not accept writes"
    );

    activate(&client, &token).await;
    assert_eq!(
        check_account_status(&client, &token).await["activated"],
        true
    );
    assert_eq!(
        create_record(
            &client,
            &token,
            "two",
            json!({"$type": "com.example.record", "text": "two"})
        )
        .await,
        Status::Ok
    );
    assert_eq!(
        check_account_status(&client, &token).await["indexedRecords"],
        2
    );
}

// ---------------------------------------------------------------------------
// Migration round trip
// ---------------------------------------------------------------------------

/// Export from one server, import into a second one holding the same DID,
/// activate it there. The repo has to arrive identical: same commit, same
/// rev, same block count, same record CIDs. Anything less is a fork of the
/// account rather than a move of it.
#[tokio::test]
async fn a_repo_exported_from_one_server_imports_into_another_unchanged() {
    let _plc = published_key_lock().await;
    let (_dir, source) = common::get_client().await;
    let source_token = account(&source).await;
    let car = seed_and_export(&source, &source_token).await;
    let source_status = check_account_status(&source, &source_token).await;

    let parsed = rsky_repo::car::read_car(car.clone())
        .await
        .expect("a parseable CAR");
    assert_eq!(parsed.roots.len(), 1);
    assert_eq!(parsed.roots[0].to_string(), source_status["repoCommit"]);
    assert_eq!(parsed.blocks.size() as i64, source_status["repoBlocks"]);

    let source_records = list_records(&source).await;

    let (_dir2, target) = common::get_client().await;
    let target_token = account(&target).await;
    let before = check_account_status(&target, &target_token).await;
    assert_eq!(before["activated"], false);
    assert_eq!(before["indexedRecords"], 0);

    assert_eq!(
        import_repo(&target, &target_token, &car).await.status(),
        Status::Ok
    );

    let imported = check_account_status(&target, &target_token).await;
    assert_eq!(imported["activated"], false, "import must not activate");
    assert_eq!(imported["repoCommit"], source_status["repoCommit"]);
    assert_eq!(imported["repoRev"], source_status["repoRev"]);
    assert_eq!(imported["repoBlocks"], source_status["repoBlocks"]);
    assert_eq!(imported["indexedRecords"], 2);

    deactivate(&source, &source_token).await;
    activate(&target, &target_token).await;
    let live = check_account_status(&target, &target_token).await;
    assert_eq!(live["activated"], true);
    assert_eq!(live["validDid"], true);
    assert_eq!(live["repoCommit"], source_status["repoCommit"]);

    assert_eq!(list_records(&target).await, source_records);

    let response = target
        .get(format!(
            "/xrpc/com.atproto.sync.getRecord?did={DID}&collection=com.example.record&rkey=one"
        ))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    assert_eq!(
        response.headers().get_one("Content-Type"),
        Some("application/vnd.ipld.car")
    );
    assert!(!response.into_bytes().await.unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Divergences from the protocol, asserted as the protocol defines them.
// ---------------------------------------------------------------------------

/// `com.atproto.repo.listMissingBlobs` names each entry's record under
/// `recordUri`. This is the endpoint a migration uses to find the blobs it
/// still has to carry; a client reading the declared field cannot locate the
/// record to re-upload for, and the blobs are silently left behind.
#[tokio::test]
#[ignore = "divergence: rsky-lexicon/src/com/atproto/repo.rs record_uri lacks serde rename to recordUri"]
async fn list_missing_blobs_names_each_blob_with_a_record_uri() {
    let _plc = published_key_lock().await;
    let (_dir, source) = common::get_client().await;
    let source_token = account(&source).await;
    let car = seed_and_export(&source, &source_token).await;

    let (_dir2, target) = common::get_client().await;
    let target_token = account(&target).await;
    assert_eq!(
        import_repo(&target, &target_token, &car).await.status(),
        Status::Ok
    );

    let response = target
        .get("/xrpc/com.atproto.repo.listMissingBlobs?limit=10")
        .header(bearer(&target_token))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body = json_body(response).await;
    let blobs = body["blobs"].as_array().unwrap();
    assert_eq!(blobs.len(), 1, "{body}");
    assert_eq!(
        blobs[0]["recordUri"],
        format!("at://{DID}/com.example.record/one"),
        "{body}"
    );
    assert_eq!(body["cursor"], blobs[0]["cid"]);
}

/// Every method a migration drives is authenticated, and an unauthenticated
/// call is `AuthMissing` at 401. That is what tells a client to re-authenticate
/// rather than to treat its own request as malformed and give up.
#[tokio::test]
#[ignore = "divergence: auth_verifier.rs AuthMissing is a bare anyhow error, surfaces as 400 InvalidRequest"]
async fn migration_endpoints_answer_an_unauthenticated_call_with_auth_missing() {
    let (_dir, client) = common::get_client().await;
    account(&client).await;

    for path in [
        "/xrpc/com.atproto.server.checkAccountStatus",
        "/xrpc/com.atproto.repo.listMissingBlobs",
    ] {
        let response = client.get(path).dispatch().await;
        assert_eq!(response.status(), Status::Unauthorized, "{path}");
        assert_eq!(json_body(response).await["error"], "AuthMissing", "{path}");
    }

    let response = client
        .post("/xrpc/com.atproto.server.activateAccount")
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Unauthorized);
    assert_eq!(json_body(response).await["error"], "AuthMissing");

    let response = client
        .post("/xrpc/com.atproto.server.deactivateAccount")
        .header(ContentType::JSON)
        .body("{}")
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Unauthorized);
    assert_eq!(json_body(response).await["error"], "AuthMissing");

    let response = client
        .post("/xrpc/com.atproto.repo.importRepo")
        .header(car_type())
        .header(Header::new("Content-Length", "4"))
        .body(vec![0u8; 4])
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Unauthorized);
    assert_eq!(json_body(response).await["error"], "AuthMissing");
}

/// A write to an account that has moved away must name `AccountDeactivated`.
/// That name is what a client branches on to stop writing to the old host and
/// follow the account; buried in a message it reads as a generic bad request.
#[tokio::test]
#[ignore = "divergence: apis/mod.rs catch-all renames AccountDeactivated to InvalidRequest"]
async fn a_write_to_a_deactivated_account_names_account_deactivated() {
    let _plc = published_key_lock().await;
    let (_dir, client) = common::get_client().await;
    let token = account(&client).await;
    activate(&client, &token).await;
    deactivate(&client, &token).await;

    let response = client
        .post("/xrpc/com.atproto.repo.createRecord")
        .header(ContentType::JSON)
        .header(bearer(&token))
        .body(
            json!({
                "repo": DID,
                "collection": "com.example.record",
                "rkey": "blocked",
                "validate": false,
                "record": {"$type": "com.example.record", "text": "blocked"},
            })
            .to_string(),
        )
        .dispatch()
        .await;
    assert_ne!(response.status(), Status::Ok);
    assert_eq!(json_body(response).await["error"], "AccountDeactivated");
}

/// A migration client points these at the source host before it knows the
/// account moved. `RepoNotFound` tells it to look elsewhere; a 500 tells it to
/// retry a broken server forever.
#[tokio::test]
#[ignore = "divergence: get_blocks/get_record/get_repo collapse RepoNotFound into 500 RuntimeError"]
async fn sync_endpoints_name_repo_not_found_for_an_unknown_did() {
    let (_dir, client) = common::get_client().await;

    for path in [
        format!("/xrpc/com.atproto.sync.getRepo?did={UNKNOWN_DID}"),
        format!("/xrpc/com.atproto.sync.getBlocks?did={UNKNOWN_DID}&cids={ABSENT_CID}"),
        format!(
            "/xrpc/com.atproto.sync.getRecord?did={UNKNOWN_DID}\
             &collection=com.example.record&rkey=one"
        ),
    ] {
        let response = client.get(path.clone()).dispatch().await;
        assert_eq!(response.status(), Status::BadRequest, "{path}");
        assert_eq!(json_body(response).await["error"], "RepoNotFound", "{path}");
    }
}

/// The source account is deactivated for the whole second half of a
/// migration. Callers still reaching it must be told the repo changed state,
/// by the name the lexicon declares, so they resolve the identity again.
#[tokio::test]
#[ignore = "divergence: get_blocks/get_record/get_repo collapse RepoDeactivated into 500 RuntimeError"]
async fn sync_endpoints_name_repo_deactivated_for_a_deactivated_account() {
    let _plc = published_key_lock().await;
    let (_dir, client) = common::get_client().await;
    let token = account(&client).await;
    activate(&client, &token).await;
    assert_eq!(
        create_record(
            &client,
            &token,
            "one",
            json!({"$type": "com.example.record", "text": "one"})
        )
        .await,
        Status::Ok
    );
    deactivate(&client, &token).await;

    for path in [
        format!("/xrpc/com.atproto.sync.getRepo?did={DID}"),
        format!(
            "/xrpc/com.atproto.sync.getRecord?did={DID}&collection=com.example.record&rkey=one"
        ),
    ] {
        let response = client.get(path.clone()).dispatch().await;
        assert_eq!(response.status(), Status::BadRequest, "{path}");
        assert_eq!(
            json_body(response).await["error"],
            "RepoDeactivated",
            "{path}"
        );
    }
}

/// `cids` is a required parameter of `com.atproto.sync.getBlocks`. Answering a
/// request that names none with an empty CAR reports "none of those blocks
/// exist" to a caller that asked for nothing.
#[tokio::test]
#[ignore = "divergence: get_blocks.rs types cids as Vec<String>, so an absent required param yields 200"]
async fn sync_get_blocks_rejects_a_request_that_names_no_cids() {
    let _plc = published_key_lock().await;
    let (_dir, client) = common::get_client().await;
    let token = account(&client).await;
    activate(&client, &token).await;

    let response = client
        .get(format!("/xrpc/com.atproto.sync.getBlocks?did={DID}"))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::BadRequest);
    assert_eq!(json_body(response).await["error"], "InvalidRequest");
}

/// A block the repo does not hold is `BlockNotFound`, the error the lexicon
/// declares. An importing server walking a repo it is fetching block by block
/// has to distinguish "that block is not here" from "this server is broken".
#[tokio::test]
#[ignore = "divergence: get_blocks.rs collapses BlockNotFound into 500 RuntimeError"]
async fn sync_get_blocks_names_block_not_found_for_a_cid_the_repo_lacks() {
    let _plc = published_key_lock().await;
    let (_dir, client) = common::get_client().await;
    let token = account(&client).await;
    activate(&client, &token).await;

    let response = client
        .get(format!(
            "/xrpc/com.atproto.sync.getBlocks?did={DID}&cids={ABSENT_CID}"
        ))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::BadRequest);
    assert_eq!(json_body(response).await["error"], "BlockNotFound");
}

/// A CAR that cannot be parsed is the client's error. Reported as a 500 it
/// reads as a transient server fault, and a migration tool retries a truncated
/// upload forever instead of re-exporting it.
#[tokio::test]
#[ignore = "divergence: import_repo.rs returns 500 RuntimeError for a short upload instead of 400"]
async fn import_repo_rejects_a_truncated_car_as_a_client_error() {
    let _plc = published_key_lock().await;
    let (_dir, source) = common::get_client().await;
    let source_token = account(&source).await;
    let car = seed_and_export(&source, &source_token).await;

    let (_dir2, target) = common::get_client().await;
    let target_token = account(&target).await;

    let response = import_repo(&target, &target_token, &car[..car.len() / 2]).await;
    assert_eq!(response.status(), Status::BadRequest);
    assert_eq!(json_body(response).await["error"], "InvalidRequest");
}
