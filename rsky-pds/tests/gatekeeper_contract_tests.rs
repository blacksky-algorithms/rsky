//! The gatekeeper writes its own rows into the shared `email_token` table
//! (purpose `2fa_code`, a timestamp in its own format) and deletes every
//! token for an account when a login completes. This server must run its
//! token flows beside those rows without ever reading them, and must
//! remove them where the reference does.

mod common;

use common::{create_account, get_admin_token, get_client_in};
use rocket::http::{ContentType, Header, Status};
use rocket::local::asynchronous::Client;
use rusqlite::Connection;
use serde_json::{json, Value};

const GATEKEEPER_PURPOSE: &str = "2fa_code";
const GATEKEEPER_STAMP: &str = "2026-09-12 20:00:00.123456 UTC";

fn insert_gatekeeper_token(db: &std::path::Path, did: &str) {
    Connection::open(db)
        .unwrap()
        .execute(
            "INSERT INTO email_token (purpose, did, token, \"requestedAt\") VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT (purpose, did) DO UPDATE SET token = excluded.token, \"requestedAt\" = excluded.\"requestedAt\"",
            rusqlite::params![GATEKEEPER_PURPOSE, did, "12345", GATEKEEPER_STAMP],
        )
        .unwrap();
}

fn tokens(db: &std::path::Path, did: &str) -> Vec<(String, String)> {
    let conn = Connection::open(db).unwrap();
    let mut statement = conn
        .prepare("SELECT purpose, token FROM email_token WHERE did = ?1 ORDER BY purpose")
        .unwrap();
    statement
        .query_map([did], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

async fn post(client: &Client, path: &str, body: Value, auth: Option<&str>) -> (Status, Value) {
    let mut request = client
        .post(path)
        .header(ContentType::JSON)
        .body(body.to_string());
    if let Some(auth) = auth {
        request = request.header(Header::new("Authorization", auth.to_owned()));
    }
    let response = request.dispatch().await;
    let status = response.status();
    (status, response.into_json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn gatekeeper_rows_never_disturb_this_servers_token_flows() {
    let dir = tempfile::tempdir().unwrap();
    let client = get_client_in(dir.path()).await;
    let (email, password) = create_account(&client).await;
    let db = dir.path().join("account.sqlite");
    let did: String = Connection::open(&db)
        .unwrap()
        .query_row(
            "SELECT did FROM account WHERE email = ?1",
            [&email],
            |row| row.get(0),
        )
        .unwrap();
    Connection::open(&db)
        .unwrap()
        .execute(
            "UPDATE actor SET \"deactivatedAt\" = NULL, \"deleteAfter\" = NULL WHERE did = ?1",
            [&did],
        )
        .unwrap();
    insert_gatekeeper_token(&db, &did);

    // a login with the gatekeeper's row present, in its own timestamp format
    let (status, session) = post(
        &client,
        "/xrpc/com.atproto.server.createSession",
        json!({"identifier": email, "password": password}),
        None,
    )
    .await;
    assert_eq!(status, Status::Ok, "{session}");
    let bearer = format!("Bearer {}", session["accessJwt"].as_str().unwrap());

    // a password reset is requested and completed beside the row; the reset
    // removes its own token and nothing else, as the reference does
    let (status, body) = post(
        &client,
        "/xrpc/com.atproto.server.requestPasswordReset",
        json!({"email": email}),
        None,
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
    let issued = tokens(&db, &did);
    assert_eq!(issued.len(), 2, "{issued:?}");
    let reset_token = issued
        .iter()
        .find(|(purpose, _)| purpose == "reset_password")
        .map(|(_, token)| token.clone())
        .unwrap();
    assert!(issued
        .iter()
        .any(|(purpose, token)| purpose == GATEKEEPER_PURPOSE && token == "12345"));
    let (status, body) = post(
        &client,
        "/xrpc/com.atproto.server.resetPassword",
        json!({"token": reset_token, "password": "a-new-password-1"}),
        None,
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
    assert_eq!(
        tokens(&db, &did),
        vec![(GATEKEEPER_PURPOSE.to_owned(), "12345".to_owned())],
        "the reset removed only its own token"
    );

    // an email confirmation completes beside the row and leaves it alone
    let (status, body) = post(
        &client,
        "/xrpc/com.atproto.server.requestEmailConfirmation",
        json!({}),
        Some(&bearer),
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
    let confirm_token = tokens(&db, &did)
        .into_iter()
        .find(|(purpose, _)| purpose == "confirm_email")
        .map(|(_, token)| token)
        .unwrap();
    let (status, body) = post(
        &client,
        "/xrpc/com.atproto.server.confirmEmail",
        json!({"email": email, "token": confirm_token}),
        Some(&bearer),
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
    assert_eq!(
        tokens(&db, &did),
        vec![(GATEKEEPER_PURPOSE.to_owned(), "12345".to_owned())]
    );

    // the gatekeeper's token is never accepted for one of this server's purposes
    let (status, body) = post(
        &client,
        "/xrpc/com.atproto.server.resetPassword",
        json!({"token": "12345", "password": "another-password-1"}),
        None,
    )
    .await;
    assert_eq!(status, Status::BadRequest, "{body}");
    assert_eq!(body["error"], "InvalidToken");

    // the gatekeeper deleting every token for the account (its login step)
    // leaves this server's flows unaffected: a new request issues a new token
    Connection::open(&db)
        .unwrap()
        .execute("DELETE FROM email_token WHERE did = ?1", [&did])
        .unwrap();
    let (status, _) = post(
        &client,
        "/xrpc/com.atproto.server.requestEmailConfirmation",
        json!({}),
        Some(&bearer),
    )
    .await;
    assert_eq!(status, Status::Ok);
    assert_eq!(tokens(&db, &did).len(), 1);

    // the admin route the gatekeeper calls stays answerable with its auth
    let (status, _) = post(
        &client,
        "/xrpc/com.atproto.server.createInviteCode",
        json!({"useCount": 1}),
        Some(&get_admin_token()),
    )
    .await;
    assert_eq!(status, Status::Ok);
}
