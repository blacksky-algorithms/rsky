use rocket::http::{ContentType, Header, Status};
use rocket::local::asynchronous::Client;
use rsky_lexicon::com::atproto::server::CreateInviteCodeOutput;
use rsky_pds::config::ServerConfig;
use rsky_pds::SharedIdResolver;
use serde_json::json;

mod common;

use crate::common::{get_admin_token, set_published_signing_key};

const PNG: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 1, 2, 3, 4];

fn handle_domain(client: &Client) -> String {
    client
        .rocket()
        .state::<ServerConfig>()
        .unwrap()
        .identity
        .service_handle_domains
        .first()
        .unwrap()
        .clone()
}

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

/// Creates an account and returns its full-access token.
async fn account(client: &Client, did: &str, name: &str) -> String {
    let invite = invite_code(client).await;
    let response = client
        .post("/xrpc/com.atproto.server.createAccount")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", get_admin_token()))
        .body(
            json!({
                "did": did,
                "email": format!("{name}@example.com"),
                "handle": format!("{name}{}", handle_domain(client)),
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

async fn app_password_session(client: &Client, full_token: &str, name: &str) -> String {
    let response = client
        .post("/xrpc/com.atproto.server.createAppPassword")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", format!("Bearer {full_token}")))
        .body(json!({ "name": "video uploads" }).to_string())
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: serde_json::Value = response.into_json().await.unwrap();
    let app_password = body["password"].as_str().unwrap().to_string();

    let response = client
        .post("/xrpc/com.atproto.server.createSession")
        .header(ContentType::JSON)
        .body(
            json!({
                "identifier": format!("{name}{}", handle_domain(client)),
                "password": app_password,
            })
            .to_string(),
        )
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: serde_json::Value = response.into_json().await.unwrap();
    body["accessJwt"].as_str().unwrap().to_string()
}

async fn service_auth(client: &Client, token: &str, aud: &str, lxm: &str) -> (Status, String) {
    let response = client
        .get(format!(
            "/xrpc/com.atproto.server.getServiceAuth?aud={aud}&lxm={lxm}"
        ))
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .dispatch()
        .await;
    let status = response.status();
    let body: serde_json::Value = response.into_json().await.unwrap();
    (
        status,
        body["token"].as_str().unwrap_or_default().to_string(),
    )
}

async fn upload_blob(client: &Client, bearer: &str) -> Status {
    client
        .post("/xrpc/com.atproto.repo.uploadBlob")
        .header(ContentType::PNG)
        .header(Header::new("Authorization", format!("Bearer {bearer}")))
        .body(PNG)
        .dispatch()
        .await
        .status()
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

/// An app-password session is what most clients hold, and it must be able
/// to mint a service token for a non-privileged method such as a video
/// service's upload-limit check.
#[tokio::test]
async fn app_password_session_can_mint_service_auth_for_a_video_service() {
    let (_dir, client) = common::get_client().await;
    let full_token = account(&client, "did:plc:gggggggggggggggggggggggg", "grace").await;
    let token = app_password_session(&client, &full_token, "grace").await;

    let (status, service_jwt) = service_auth(
        &client,
        &token,
        "did:web:video.invalid",
        "app.bsky.video.getUploadLimits",
    )
    .await;
    assert_eq!(status, Status::Ok);
    assert!(!service_jwt.is_empty());

    // protected methods are never delegated
    let (status, _) = service_auth(
        &client,
        &token,
        "did:web:video.invalid",
        "com.atproto.server.createAppPassword",
    )
    .await;
    assert_ne!(status, Status::Ok);
}

/// A remote service holding a token the account minted for `uploadBlob`
/// at this PDS can upload on the account's behalf; the same token is
/// refused for any other method or audience, and an ordinary session still
/// uploads as before.
#[tokio::test]
async fn a_service_token_minted_for_upload_blob_uploads_a_blob() {
    let (_dir, client) = common::get_client().await;
    let did = "did:plc:hhhhhhhhhhhhhhhhhhhhhhhh";
    let token = account(&client, did, "heidi").await;
    set_published_signing_key(Some(recommended_signing_key(&client, &token).await));
    // Account creation already resolved and cached the document, before the
    // key above was published to it.
    let doc = client
        .rocket()
        .state::<SharedIdResolver>()
        .unwrap()
        .id_resolver
        .write()
        .await
        .did
        .ensure_resolve(&did.to_string(), Some(true))
        .await
        .unwrap();
    assert!(doc.verification_method.is_some_and(|keys| !keys.is_empty()));
    let pds_did = std::env::var("PDS_SERVICE_DID").unwrap();

    let (status, service_jwt) =
        service_auth(&client, &token, &pds_did, "com.atproto.repo.uploadBlob").await;
    assert_eq!(status, Status::Ok);
    assert_eq!(upload_blob(&client, &service_jwt).await, Status::Ok);

    let (status, other_method) =
        service_auth(&client, &token, &pds_did, "app.bsky.feed.getTimeline").await;
    assert_eq!(status, Status::Ok);
    assert_ne!(upload_blob(&client, &other_method).await, Status::Ok);

    let (status, other_audience) = service_auth(
        &client,
        &token,
        "did:web:appview.invalid",
        "com.atproto.repo.uploadBlob",
    )
    .await;
    assert_eq!(status, Status::Ok);
    assert_ne!(upload_blob(&client, &other_audience).await, Status::Ok);

    assert_eq!(upload_blob(&client, &token).await, Status::Ok);
    assert_ne!(upload_blob(&client, "not.a.jwt").await, Status::Ok);
    set_published_signing_key(None);
}
