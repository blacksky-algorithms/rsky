use rocket::http::{ContentType, Header, Status};
use rocket::local::asynchronous::Client;
use rsky_crypto::utils::encode_did_key;
use rsky_lexicon::com::atproto::server::CreateInviteCodeOutput;
use rsky_pds::config::ServerConfig;
use rsky_pds::context::PDS_REPO_SIGNING_KEYPAIR;
use rsky_pds::xrpc_server::auth::verify_jwt;
use serde_json::json;

mod common;

use crate::common::{get_admin_token, set_published_signing_key};

async fn invite_code(client: &Client) -> String {
    client
        .post("/xrpc/com.atproto.server.createInviteCode")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", get_admin_token()))
        .body(json!({ "useCount": 1 }).to_string())
        .dispatch()
        .await
        .into_json::<CreateInviteCodeOutput>()
        .await
        .unwrap()
        .code
}

/// Creates an account with a caller-supplied did and returns its access token.
async fn account(client: &Client, did: &str, name: &str) -> String {
    let domain = client
        .rocket()
        .state::<ServerConfig>()
        .unwrap()
        .identity
        .service_handle_domains
        .first()
        .unwrap()
        .clone();
    let invite = invite_code(client).await;
    let response = client
        .post("/xrpc/com.atproto.server.createAccount")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", get_admin_token()))
        .body(
            json!({
                "did": did,
                "email": format!("{name}@example.com"),
                "handle": format!("{name}{domain}"),
                "password": "password",
                "inviteCode": invite,
            })
            .to_string(),
        )
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: serde_json::Value = response.into_json().await.unwrap();
    body["accessJwt"].as_str().unwrap().to_string()
}

async fn recommended_signing_key(client: &Client, token: &str) -> String {
    let response = client
        .get("/xrpc/com.atproto.identity.getRecommendedDidCredentials")
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: serde_json::Value = response.into_json().await.unwrap();
    body["verificationMethods"]["atproto"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Two accounts must not share a signing key, and neither may be the
/// process-wide `PDS_REPO_SIGNING_KEYPAIR` every account used to get.
#[tokio::test]
async fn each_account_gets_its_own_signing_key() {
    let (_dir, client) = common::get_client().await;
    let alice = account(&client, "did:plc:aaaaaaaaaaaaaaaaaaaaaaaa", "alice").await;
    let bob = account(&client, "did:plc:bbbbbbbbbbbbbbbbbbbbbbbb", "bob").await;

    let alice_key = recommended_signing_key(&client, &alice).await;
    let bob_key = recommended_signing_key(&client, &bob).await;
    let shared_key = encode_did_key(&PDS_REPO_SIGNING_KEYPAIR.public_key());

    assert_ne!(alice_key, bob_key);
    assert_ne!(alice_key, shared_key);
    assert_ne!(bob_key, shared_key);
}

/// A `getServiceAuth` token must verify against the issuing account's own
/// key — the property a remote service checks after resolving `iss`.
#[tokio::test]
async fn a_service_auth_token_verifies_against_the_issuing_accounts_key() {
    let (_dir, client) = common::get_client().await;
    let did = "did:plc:cccccccccccccccccccccccc";
    let token = account(&client, did, "carol").await;
    let signing_key = recommended_signing_key(&client, &token).await;

    let response = client
        .get(
            "/xrpc/com.atproto.server.getServiceAuth\
             ?aud=did:web:appview.invalid&lxm=app.bsky.feed.getTimeline",
        )
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: serde_json::Value = response.into_json().await.unwrap();
    let service_jwt = body["token"].as_str().unwrap().to_string();

    let payload = verify_jwt(
        service_jwt.clone(),
        Some("did:web:appview.invalid".to_string()),
        Some("app.bsky.feed.getTimeline"),
        |_iss, _refresh| {
            let signing_key = signing_key.clone();
            async move { Ok(signing_key) }
        },
    )
    .await
    .unwrap();
    assert_eq!(payload.iss, did);

    // the old shared key must no longer verify it
    let shared_key = encode_did_key(&PDS_REPO_SIGNING_KEYPAIR.public_key());
    let error = verify_jwt(
        service_jwt,
        Some("did:web:appview.invalid".to_string()),
        Some("app.bsky.feed.getTimeline"),
        move |_iss, _refresh| {
            let shared_key = shared_key.clone();
            async move { Ok(shared_key) }
        },
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(error.starts_with("BadJwtSignature"), "{error}");
}

/// `checkAccountStatus` reports `validDid` by comparing the published
/// document against the account's own key, not a server-wide one.
#[tokio::test]
async fn the_did_document_is_validated_against_the_accounts_own_key() {
    let (_dir, client) = common::get_client().await;
    let did = "did:plc:dddddddddddddddddddddddd";
    let token = account(&client, did, "dave").await;
    let signing_key = recommended_signing_key(&client, &token).await;

    let valid_did = |token: String| {
        let client = &client;
        async move {
            let response = client
                .get("/xrpc/com.atproto.server.checkAccountStatus")
                .header(Header::new("Authorization", format!("Bearer {token}")))
                .dispatch()
                .await;
            assert_eq!(response.status(), Status::Ok);
            let body: serde_json::Value = response.into_json().await.unwrap();
            body["validDid"].as_bool().unwrap()
        }
    };

    set_published_signing_key(Some(signing_key));
    assert!(valid_did(token.clone()).await);

    // the process-wide key is no longer what the document must name
    set_published_signing_key(Some(encode_did_key(&PDS_REPO_SIGNING_KEYPAIR.public_key())));
    assert!(!valid_did(token).await);
    set_published_signing_key(None);
}

/// A full-access session (ordinary password login, `AuthScope::Access`) is
/// privileged and must be allowed to mint a service-auth token for a
/// privileged `chat.bsky.*` method.
///
/// This is an end-to-end regression test for `Credentials.is_privileged`
/// actually being populated from the session's real scope inside
/// `validate_access_token` / `validate_dpop_access_token`
/// (`auth_verifier.rs`), rather than being hardcoded to `None`. That bug sat
/// upstream of `getServiceAuth`'s own privilege check: with `is_privileged`
/// always `None` (-> `false`), a corrected `getServiceAuth` condition would
/// deny *every* privileged-method request, including this one, regardless of
/// session type. A unit test of the extracted `ensure_lxm_access` helper
/// alone cannot catch this, because the bug is in how its `is_privileged`
/// input gets constructed, not in the helper itself.
#[tokio::test]
async fn full_access_session_is_allowed_service_auth_for_a_privileged_chat_method() {
    let (_dir, client) = common::get_client().await;
    let token = account(&client, "did:plc:eeeeeeeeeeeeeeeeeeeeeeee", "erin").await;

    let response = client
        .get(
            "/xrpc/com.atproto.server.getServiceAuth\
             ?aud=did:web:appview.invalid&lxm=chat.bsky.convo.getMessages",
        )
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: serde_json::Value = response.into_json().await.unwrap();
    assert!(body["token"].as_str().is_some());
}

/// A plain app-password session (`AuthScope::AppPass`, not privileged) must
/// be denied a service-auth token for a privileged `chat.bsky.*` method,
/// even though the same account's full-access session is allowed one above.
#[tokio::test]
async fn app_password_session_is_denied_service_auth_for_a_privileged_chat_method() {
    let (_dir, client) = common::get_client().await;
    let full_token = account(&client, "did:plc:ffffffffffffffffffffffff", "frank").await;

    let response = client
        .post("/xrpc/com.atproto.server.createAppPassword")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", format!("Bearer {full_token}")))
        .body(json!({ "name": "test app password" }).to_string())
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: serde_json::Value = response.into_json().await.unwrap();
    let app_password = body["password"].as_str().unwrap().to_string();

    let domain = client
        .rocket()
        .state::<ServerConfig>()
        .unwrap()
        .identity
        .service_handle_domains
        .first()
        .unwrap()
        .clone();
    let response = client
        .post("/xrpc/com.atproto.server.createSession")
        .header(ContentType::JSON)
        .body(
            json!({
                "identifier": format!("frank{domain}"),
                "password": app_password,
            })
            .to_string(),
        )
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: serde_json::Value = response.into_json().await.unwrap();
    let app_password_token = body["accessJwt"].as_str().unwrap().to_string();

    let response = client
        .get(
            "/xrpc/com.atproto.server.getServiceAuth\
             ?aud=did:web:appview.invalid&lxm=chat.bsky.convo.getMessages",
        )
        .header(Header::new(
            "Authorization",
            format!("Bearer {app_password_token}"),
        ))
        .dispatch()
        .await;
    assert_ne!(
        response.status(),
        Status::Ok,
        "a plain app-password session must not be able to mint a service-auth \
         token for a privileged chat.bsky.* method"
    );
}
