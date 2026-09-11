//! Regression tests for expired-token error surfacing.
//!
//! An expired access or refresh token must render as
//! `400 {"error":"ExpiredToken","message":"Token has expired"}` so clients can
//! distinguish an expired session (call refreshSession / re-login) from a
//! genuinely malformed token (`InvalidRequest`). Historically every auth guard
//! collapsed both cases into `InvalidRequest`, leaving clients unable to tell
//! the two apart.

use rocket::http::{ContentType, Header, Status};
use rocket::local::asynchronous::Client;
use rocket::serde::json::json;
use rsky_pds::account_manager::helpers::auth::{
    create_access_token_with, create_refresh_token_with, now_secs, CreateTokensOpts, PDS_JWT_SIGNER,
};
use rsky_pds::auth_verifier::AuthScope;

mod common;

/// Signs an already-EXPIRED JWT with the PDS's own signing key so it passes
/// signature verification and fails purely on the expiry claim: issued three
/// hours ago with the standard two-hour lifetime, so it expired an hour ago.
fn sign_expired_token(scope: AuthScope, did: &str, aud: &str, jti: Option<String>) -> String {
    let opts = CreateTokensOpts {
        did: did.to_owned(),
        service_did: aud.to_owned(),
        scope: Some(scope.clone()),
        jti,
        expires_in_secs: Some(2 * 60 * 60),
        issued_at: Some(now_secs() - 3 * 60 * 60),
    };
    match scope {
        AuthScope::Refresh => create_refresh_token_with(&PDS_JWT_SIGNER, opts),
        _ => create_access_token_with(&PDS_JWT_SIGNER, opts),
    }
    .expect("sign token with the PDS signer")
}

fn service_did() -> String {
    std::env::var("PDS_SERVICE_DID").expect("PDS_SERVICE_DID set by test env")
}

const TEST_DID: &str = "did:plc:khvyd3oiw46vif5gm7hijslk";

/// Creates an account and returns a genuine, unexpired access token minted by
/// the server's own createSession flow (not one we sign by hand).
async fn real_access_token(client: &Client) -> String {
    let (identifier, password) = common::create_account(client).await;
    let response = client
        .post("/xrpc/com.atproto.server.createSession")
        .header(ContentType::JSON)
        .body(json!({ "identifier": identifier, "password": password }).to_string())
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: serde_json::Value = response.into_json().await.expect("session json");
    body["accessJwt"].as_str().expect("accessJwt").to_string()
}

/// DoD 1: an expired ACCESS token yields 400 ExpiredToken.
#[tokio::test]
async fn expired_access_token_returns_expired_token() {
    let (_dir, client) = common::get_client().await;
    let aud = service_did();
    let token = sign_expired_token(AuthScope::Access, TEST_DID, &aud, None);

    let response = client
        .get("/xrpc/com.atproto.server.getSession")
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .dispatch()
        .await;

    let status = response.status();
    let body: serde_json::Value = response.into_json().await.expect("json error body");
    assert_eq!(status, Status::BadRequest, "body was {body}");
    assert_eq!(body["error"], "ExpiredToken", "body was {body}");
    assert_eq!(body["message"], "Token has expired", "body was {body}");
}

/// DoD 2: an expired REFRESH token on refreshSession yields 400 ExpiredToken.
#[tokio::test]
async fn expired_refresh_token_returns_expired_token() {
    let (_dir, client) = common::get_client().await;
    let aud = service_did();
    let token = sign_expired_token(
        AuthScope::Refresh,
        TEST_DID,
        &aud,
        Some("refresh-jti-123".to_string()),
    );

    let response = client
        .post("/xrpc/com.atproto.server.refreshSession")
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .dispatch()
        .await;

    let status = response.status();
    let body: serde_json::Value = response.into_json().await.expect("json error body");
    assert_eq!(status, Status::BadRequest, "body was {body}");
    assert_eq!(body["error"], "ExpiredToken", "body was {body}");
    assert_eq!(body["message"], "Token has expired", "body was {body}");
}

/// DoD 3: a malformed token must NOT be reported as ExpiredToken -- it is
/// InvalidToken so the two failure modes remain distinguishable.
#[tokio::test]
async fn malformed_token_returns_invalid_token_not_expired() {
    let (_dir, client) = common::get_client().await;

    let response = client
        .get("/xrpc/com.atproto.server.getSession")
        .header(Header::new("Authorization", "Bearer not.a.valid.jwt"))
        .dispatch()
        .await;

    let status = response.status();
    let body: serde_json::Value = response.into_json().await.expect("json error body");
    assert_eq!(status, Status::BadRequest, "body was {body}");
    assert_ne!(body["error"], "ExpiredToken", "body was {body}");
    assert_eq!(body["error"], "InvalidToken", "body was {body}");
    assert_eq!(
        body["message"], "Token could not be verified",
        "body was {body}"
    );
}

/// DoD 4: a valid, unexpired access token for an existing account still
/// authenticates and returns 200 -- no regression from routing auth errors
/// through the new mapping. Uses `getRecommendedDidCredentials`, an
/// `AccessStandard`-guarded endpoint that resolves accounts with
/// `include_deactivated` and needs no repo/actor store, so it returns 200 for
/// the freshly (admin-)created account without extra setup.
#[tokio::test]
async fn valid_access_token_still_authenticates() {
    let (_dir, client) = common::get_client().await;
    let token = real_access_token(&client).await;

    let response = client
        .get("/xrpc/com.atproto.identity.getRecommendedDidCredentials")
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .dispatch()
        .await;

    assert_eq!(response.status(), Status::Ok);
}
