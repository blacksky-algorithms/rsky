//! Account deletion and email update through the public routes, on a
//! server that owns its blob storage (no coexistence).

mod common;

use common::{create_account, get_admin_token, get_client};
use rocket::http::{ContentType, Header, Status};
use rocket::local::asynchronous::Client;
use rocket::serde::json::json;
use serde_json::Value;

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
    body["accessJwt"].as_str().unwrap().to_owned()
}

fn account_db(dir: &std::path::Path) -> rusqlite::Connection {
    rusqlite::Connection::open(dir.join("account.sqlite")).unwrap()
}

async fn post(client: &Client, path: &str, token: Option<&str>, body: Value) -> (Status, Value) {
    let mut request = client
        .post(path)
        .header(ContentType::JSON)
        .body(body.to_string());
    if let Some(token) = token {
        request = request.header(Header::new("Authorization", token.to_owned()));
    }
    let response = request.dispatch().await;
    let status = response.status();
    let text = response.into_string().await.unwrap_or_default();
    let json = if text.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap()
    };
    (status, json)
}

#[tokio::test]
async fn delete_account_through_the_public_route() {
    let (dir, client) = get_client().await;
    let (identifier, password) = create_account(&client).await;
    account_db(dir.path())
        .execute(
            "INSERT INTO email_token (purpose, did, token, \"requestedAt\") VALUES ('delete_account', ?1, 'DELETE-ME', ?2)",
            rusqlite::params![DID, rsky_common::now()],
        )
        .unwrap();
    let delete = "/xrpc/com.atproto.server.deleteAccount";
    let (status, json) = post(
        &client,
        delete,
        None,
        json!({"did": DID, "password": "wrong", "token": "DELETE-ME"}),
    )
    .await;
    assert_eq!(status, Status::Unauthorized);
    assert_eq!(json["error"], "AuthenticationRequired");
    let (status, json) = post(
        &client,
        delete,
        None,
        json!({"did": DID, "password": password, "token": "delete-me"}),
    )
    .await;
    assert_eq!(status, Status::Ok, "{json}");
    let response = client
        .get(format!("/xrpc/com.atproto.sync.getRepo?did={DID}"))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::BadRequest);
    let body: Value = response.into_json().await.unwrap();
    assert_eq!(body["error"], "RepoNotFound");
    let (status, json) = post(
        &client,
        "/xrpc/com.atproto.server.createSession",
        None,
        json!({"identifier": identifier, "password": password}),
    )
    .await;
    assert_eq!(status, Status::Unauthorized, "{json}");
    // the deletion is journaled as complete and, with its own blob storage,
    // leaves no purge obligation behind
    let lifecycle = rusqlite::Connection::open(dir.path().join("rsky/lifecycle.sqlite")).unwrap();
    let deleted: Option<String> = lifecycle
        .query_row(
            "SELECT \"logicallyDeletedAt\" FROM tombstone WHERE did = ?1",
            [DID],
            |row| row.get(0),
        )
        .unwrap();
    assert!(deleted.is_some());
    let obligations: i64 = lifecycle
        .query_row(
            "SELECT count(*) FROM purge_obligation WHERE did = ?1",
            [DID],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(obligations, 0);
}

#[tokio::test]
async fn admin_deletes_an_account() {
    let (_dir, client) = get_client().await;
    create_account(&client).await;
    let (status, json) = post(
        &client,
        "/xrpc/com.atproto.admin.deleteAccount",
        Some(&get_admin_token()),
        json!({"did": DID}),
    )
    .await;
    assert_eq!(status, Status::Ok, "{json}");
    let response = client
        .get(format!("/xrpc/com.atproto.sync.getRepo?did={DID}"))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::BadRequest);
}

#[tokio::test]
async fn update_email_follows_the_reference_rules() {
    let (dir, client) = get_client().await;
    let (identifier, password) = create_account(&client).await;
    let token = format!(
        "Bearer {}",
        session_token(&client, &identifier, &password).await
    );
    let update = "/xrpc/com.atproto.server.updateEmail";
    let (status, json) = post(
        &client,
        update,
        Some(&token),
        json!({"email": "not an email"}),
    )
    .await;
    assert_eq!(status, Status::BadRequest);
    assert_eq!(json["error"], "InvalidRequest");
    let (status, json) = post(
        &client,
        update,
        Some(&token),
        json!({"email": "renamed@example.com"}),
    )
    .await;
    assert_eq!(status, Status::Ok, "{json}");
    let response = client
        .get("/xrpc/com.atproto.server.getSession")
        .header(Header::new("Authorization", token.clone()))
        .dispatch()
        .await;
    let body: Value = response.into_json().await.unwrap();
    assert_eq!(body["email"], "renamed@example.com");
    // once the email is confirmed, a change needs the mailed token
    let db = account_db(dir.path());
    db.execute(
        "UPDATE account SET \"emailConfirmedAt\" = ?1 WHERE did = ?2",
        rusqlite::params![rsky_common::now(), DID],
    )
    .unwrap();
    let (status, json) = post(
        &client,
        update,
        Some(&token),
        json!({"email": "again@example.com"}),
    )
    .await;
    assert_eq!(status, Status::BadRequest);
    assert_eq!(json["message"], "Confirmation token required");
    let (status, json) = post(
        &client,
        update,
        Some(&token),
        json!({"email": "again@example.com", "token": "NOPE"}),
    )
    .await;
    assert_eq!(status, Status::BadRequest);
    assert_eq!(
        json,
        json!({"error": "InvalidToken", "message": "Token is invalid"})
    );
    db.execute(
        "INSERT INTO email_token (purpose, did, token, \"requestedAt\") VALUES ('update_email', ?1, 'CHANGE-IT', ?2)",
        rusqlite::params![DID, rsky_common::now()],
    )
    .unwrap();
    let (status, json) = post(
        &client,
        update,
        Some(&token),
        json!({"email": "again@example.com", "token": "change-it"}),
    )
    .await;
    assert_eq!(status, Status::Ok, "{json}");
}

/// Without a mail transport the deletion request still mints the token the
/// deletion needs and logs the message the reference would have sent.
#[tokio::test]
async fn request_account_delete_mints_a_token() {
    let (dir, client) = get_client().await;
    let (identifier, password) = create_account(&client).await;
    let token = format!(
        "Bearer {}",
        session_token(&client, &identifier, &password).await
    );
    let request = "/xrpc/com.atproto.server.requestAccountDelete";
    let response = client
        .post(request)
        .header(Header::new("Authorization", token.clone()))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let db = account_db(dir.path());
    let minted: i64 = db
        .query_row(
            "SELECT count(*) FROM email_token WHERE did = ?1 AND purpose = 'delete_account'",
            [DID],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(minted, 1);
    // an account whose row is gone but whose actor remains has no email to mail
    db.execute("DELETE FROM account WHERE did = ?1", [DID])
        .unwrap();
    let response = client
        .post(request)
        .header(Header::new("Authorization", token))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::BadRequest);
    let body: Value = response.into_json().await.unwrap();
    assert_eq!(body["message"], "account does not have an email address");
}
