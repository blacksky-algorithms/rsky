//! Integration tests for the WebFinger endpoint (RFC 7033).
//!
//! These tests verify the full HTTP request/response cycle for WebFinger,
//! including proper CORS headers, content type, and response structure.

use rocket::http::{ContentType, Header, Status};
use rocket::local::asynchronous::Client;
use rsky_lexicon::com::atproto::server::CreateInviteCodeOutput;
use rsky_pds::config::ServerConfig;
use serde_json::{json, Value};

mod common;

/// Creates an ACTIVE account (without specifying a DID, so server generates one).
/// Returns (did, handle) for the created account.
async fn create_active_account(client: &Client) -> (String, String) {
    let domain = client
        .rocket()
        .state::<ServerConfig>()
        .unwrap()
        .identity
        .service_handle_domains
        .first()
        .unwrap();

    // Create invite code
    let input = json!({ "useCount": 1 });
    let response = client
        .post("/xrpc/com.atproto.server.createInviteCode")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", common::get_admin_token()))
        .body(input.to_string())
        .dispatch()
        .await;
    let invite_code = response
        .into_json::<CreateInviteCodeOutput>()
        .await
        .unwrap()
        .code;

    // Create account WITHOUT specifying a DID - server will generate one
    // and the account will be ACTIVE (not deactivated)
    let handle = format!("webfingertest{domain}");
    let account_input = json!({
        "email": "webfingertest@example.com",
        "handle": handle,
        "password": "password123",
        "inviteCode": invite_code
    });

    let response = client
        .post("/xrpc/com.atproto.server.createAccount")
        .header(ContentType::JSON)
        .body(account_input.to_string())
        .dispatch()
        .await;

    assert_eq!(response.status(), Status::Ok);
    let body: Value = response.into_json().await.unwrap();

    let did = body["did"].as_str().unwrap().to_string();
    (did, handle)
}

/// Test WebFinger with a valid DID for an existing account.
#[tokio::test]
async fn test_webfinger_with_valid_did() {
    let (_dir, client) = common::get_client().await;
    let (did, _handle) = create_active_account(&client).await;

    let response = client
        .get(format!("/.well-known/webfinger?resource={}", did))
        .dispatch()
        .await;

    assert_eq!(response.status(), Status::Ok);

    // Verify content type
    let content_type = response.content_type();
    assert!(content_type.is_some());
    assert!(content_type
        .unwrap()
        .to_string()
        .contains("application/json"));

    // Verify CORS headers
    let cors_header = response.headers().get_one("Access-Control-Allow-Origin");
    assert_eq!(cors_header, Some("*"));

    // Verify response structure
    let body: Value = response.into_json().await.unwrap();
    assert!(body["subject"].is_string());
    assert!(body["aliases"].is_array());
    assert!(body["properties"].is_object());
    assert!(body["links"].is_array());

    // Verify the DID is in aliases
    let aliases = body["aliases"].as_array().unwrap();
    assert!(aliases.iter().any(|a| a.as_str() == Some(did.as_str())));

    // Verify properties contain the DID
    let did_prop = body["properties"]["https://atproto.com/ns/did"].as_str();
    assert_eq!(did_prop, Some(did.as_str()));
}

/// Test WebFinger with an acct-URI for an existing account.
#[tokio::test]
async fn test_webfinger_with_valid_acct_uri() {
    let (_dir, client) = common::get_client().await;
    let (_did, handle) = create_active_account(&client).await;

    // Get the server hostname
    let hostname = &client
        .rocket()
        .state::<ServerConfig>()
        .unwrap()
        .service
        .hostname;

    let response = client
        .get(format!(
            "/.well-known/webfinger?resource=acct:{}@{}",
            handle, hostname
        ))
        .dispatch()
        .await;

    assert_eq!(response.status(), Status::Ok);

    let body: Value = response.into_json().await.unwrap();

    // Subject should be acct-URI format
    let subject = body["subject"].as_str().unwrap();
    assert!(subject.starts_with("acct:"));

    // Properties should contain handle
    let handle_prop = body["properties"]["https://atproto.com/ns/handle"].as_str();
    assert!(handle_prop.is_some());
}

/// Test WebFinger with an AT Protocol URI.
#[tokio::test]
async fn test_webfinger_with_valid_at_uri() {
    let (_dir, client) = common::get_client().await;
    let (_did, handle) = create_active_account(&client).await;

    // Test with AT URI using handle
    let response = client
        .get(format!("/.well-known/webfinger?resource=at://{}", handle))
        .dispatch()
        .await;

    assert_eq!(response.status(), Status::Ok);

    let body: Value = response.into_json().await.unwrap();
    assert!(body["subject"].is_string());
    assert!(body["links"].is_array());
}

/// Test WebFinger with AT URI containing a DID.
#[tokio::test]
async fn test_webfinger_with_at_uri_did() {
    let (_dir, client) = common::get_client().await;
    let (did, _handle) = create_active_account(&client).await;

    let response = client
        .get(format!("/.well-known/webfinger?resource=at://{}", did))
        .dispatch()
        .await;

    assert_eq!(response.status(), Status::Ok);

    let body: Value = response.into_json().await.unwrap();
    assert!(body["subject"].is_string());
}

/// Test that WebFinger returns 400 for invalid resource format.
#[tokio::test]
async fn test_webfinger_invalid_resource_bare_handle() {
    let (_dir, client) = common::get_client().await;

    let response = client
        .get("/.well-known/webfinger?resource=alice.test")
        .dispatch()
        .await;

    assert_eq!(response.status(), Status::BadRequest);

    let body: Value = response.into_json().await.unwrap();
    assert_eq!(body["error"], "InvalidRequest");
    assert!(body["message"].as_str().unwrap().contains("must be a URI"));
}

/// Test that WebFinger returns 400 for empty resource.
#[tokio::test]
async fn test_webfinger_empty_resource() {
    let (_dir, client) = common::get_client().await;

    let response = client
        .get("/.well-known/webfinger?resource=")
        .dispatch()
        .await;

    assert_eq!(response.status(), Status::BadRequest);

    let body: Value = response.into_json().await.unwrap();
    assert_eq!(body["error"], "InvalidRequest");
    assert!(body["message"]
        .as_str()
        .unwrap()
        .contains("cannot be empty"));
}

/// Test that WebFinger returns 400 for malformed DID.
#[tokio::test]
async fn test_webfinger_malformed_did() {
    let (_dir, client) = common::get_client().await;

    let response = client
        .get("/.well-known/webfinger?resource=did:plc")
        .dispatch()
        .await;

    assert_eq!(response.status(), Status::BadRequest);

    let body: Value = response.into_json().await.unwrap();
    assert_eq!(body["error"], "InvalidRequest");
    assert!(body["message"]
        .as_str()
        .unwrap()
        .contains("Invalid DID format"));
}

/// Test that WebFinger returns 400 for malformed acct-URI.
#[tokio::test]
async fn test_webfinger_malformed_acct_uri() {
    let (_dir, client) = common::get_client().await;

    let response = client
        .get("/.well-known/webfinger?resource=acct:invalid")
        .dispatch()
        .await;

    assert_eq!(response.status(), Status::BadRequest);

    let body: Value = response.into_json().await.unwrap();
    assert_eq!(body["error"], "InvalidRequest");
}

/// Test that WebFinger returns 400 for non-existent DID.
#[tokio::test]
async fn test_webfinger_nonexistent_did() {
    let (_dir, client) = common::get_client().await;

    let response = client
        .get("/.well-known/webfinger?resource=did:plc:nonexistent123456")
        .dispatch()
        .await;

    assert_eq!(response.status(), Status::BadRequest);

    let body: Value = response.into_json().await.unwrap();
    assert_eq!(body["error"], "AccountNotFound");
}

/// Test that WebFinger returns 400 for host mismatch in acct-URI.
#[tokio::test]
async fn test_webfinger_host_mismatch() {
    let (_dir, client) = common::get_client().await;
    let (_did, handle) = create_active_account(&client).await;

    // Use the account's handle but with a different host that doesn't match the server
    let response = client
        .get(format!(
            "/.well-known/webfinger?resource=acct:{}@different.host.com",
            handle
        ))
        .dispatch()
        .await;

    assert_eq!(response.status(), Status::BadRequest);

    let body: Value = response.into_json().await.unwrap();
    assert_eq!(body["error"], "InvalidRequest");
    assert!(body["message"]
        .as_str()
        .unwrap()
        .contains("Host does not match"));
}

/// Test that WebFinger response includes required links.
#[tokio::test]
async fn test_webfinger_response_links() {
    let (_dir, client) = common::get_client().await;
    let (did, _handle) = create_active_account(&client).await;

    let response = client
        .get(format!("/.well-known/webfinger?resource={}", did))
        .dispatch()
        .await;

    assert_eq!(response.status(), Status::Ok);

    let body: Value = response.into_json().await.unwrap();
    let links = body["links"].as_array().unwrap();

    // Should have self link (PLC directory)
    let self_link = links.iter().find(|l| l["rel"] == "self");
    assert!(self_link.is_some());
    assert_eq!(self_link.unwrap()["type"], "application/did+ld+json");

    // Should have alternate link (repo export)
    let alternate_link = links.iter().find(|l| l["rel"] == "alternate");
    assert!(alternate_link.is_some());
    assert_eq!(alternate_link.unwrap()["type"], "application/vnd.ipld.car");

    // Should have identity resolution link
    let did_doc_link = links
        .iter()
        .find(|l| l["rel"] == "https://atproto.com/rels/did-doc");
    assert!(did_doc_link.is_some());
}

/// Test CORS headers are present for cross-origin discovery.
#[tokio::test]
async fn test_webfinger_cors_headers() {
    let (_dir, client) = common::get_client().await;
    let (did, _handle) = create_active_account(&client).await;

    let response = client
        .get(format!("/.well-known/webfinger?resource={}", did))
        .dispatch()
        .await;

    assert_eq!(response.status(), Status::Ok);

    // Check CORS headers - the global CORS fairing adds these
    // The important thing is that cross-origin requests are allowed
    let allow_origin = response.headers().get_one("Access-Control-Allow-Origin");
    assert_eq!(allow_origin, Some("*"));

    // The global fairing may set more permissive methods than just GET,
    // but GET must be included for WebFinger to work
    let allow_methods = response
        .headers()
        .get_one("Access-Control-Allow-Methods")
        .unwrap_or("");
    assert!(
        allow_methods.contains("GET"),
        "CORS must allow GET method for WebFinger"
    );
}

/// Test that WebFinger rejects HTTPS URIs (unsupported scheme).
#[tokio::test]
async fn test_webfinger_rejects_https_uri() {
    let (_dir, client) = common::get_client().await;

    let response = client
        .get("/.well-known/webfinger?resource=https://example.com/profile/alice")
        .dispatch()
        .await;

    assert_eq!(response.status(), Status::BadRequest);

    let body: Value = response.into_json().await.unwrap();
    assert_eq!(body["error"], "InvalidRequest");
}

/// Test WebFinger with URL-encoded resource parameter.
#[tokio::test]
async fn test_webfinger_url_encoded_resource() {
    let (_dir, client) = common::get_client().await;
    let (did, _handle) = create_active_account(&client).await;

    // URL-encode the colons in did:plc:...
    let encoded_did = did.replace(":", "%3A");
    let response = client
        .get(format!("/.well-known/webfinger?resource={}", encoded_did))
        .dispatch()
        .await;

    assert_eq!(response.status(), Status::Ok);

    let body: Value = response.into_json().await.unwrap();
    assert!(body["subject"].is_string());
}
