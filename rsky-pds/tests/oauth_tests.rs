use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use rocket::http::{ContentType, Header, Status};
use rocket::local::asynchronous::Client;
use rsky_oauth::jwk::{EcCurve, Jwk};
use rsky_oauth::jwt::{JwtClaims, JwtHeader};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

mod common;

const LOOPBACK_CLIENT_ID: &str =
    "http://localhost?scope=atproto%20transition%3Ageneric&redirect_uri=http%3A%2F%2F127.0.0.1%3A8080%2Fcb";
const REDIRECT_URI: &str = "http://127.0.0.1:8080/cb";

/// A loopback client_id requesting exactly `scope` -- the loopback client's
/// metadata is derived from its own URL, so `allowed_scopes` is exactly what
/// this embeds and the PAR request below must ask for the same string.
#[allow(dead_code)] // only the scoped-access-guard tests drive non-default scopes
fn loopback_client_id(scope: &str) -> String {
    format!(
        "http://localhost?scope={}&redirect_uri={}",
        url::form_urlencoded::byte_serialize(scope.as_bytes()).collect::<String>(),
        url::form_urlencoded::byte_serialize(REDIRECT_URI.as_bytes()).collect::<String>(),
    )
}
const PKCE_VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const PKCE_CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

static JTI: AtomicU64 = AtomicU64::new(0);
static OAUTH_ENV: std::sync::Once = std::sync::Once::new();

/// Pin the DPoP nonce secret before any provider is constructed so the
/// shared-secret configuration path is exercised.
async fn get_oauth_client() -> (tempfile::TempDir, Client) {
    OAUTH_ENV.call_once(|| {
        std::env::set_var(
            "PDS_DPOP_SECRET",
            "0101010101010101010101010101010101010101010101010101010101010101",
        );
    });
    common::get_client().await
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn public_url(client: &Client) -> String {
    client
        .rocket()
        .state::<rsky_pds::config::ServerConfig>()
        .unwrap()
        .service
        .public_url
        .clone()
}

fn dpop_key() -> Jwk {
    Jwk::from_private_key_bytes(EcCurve::P256, &[0x51u8; 32]).unwrap()
}

fn dpop_proof(
    key: &Jwk,
    htm: &str,
    htu: &str,
    nonce: Option<&str>,
    access_token: Option<&str>,
) -> String {
    let mut header = JwtHeader::new("ES256");
    header.typ = Some("dpop+jwt".to_string());
    header.jwk = Some(key.to_public());
    let mut claims = JwtClaims {
        iat: Some(now_secs()),
        jti: Some(format!("test-jti-{}", JTI.fetch_add(1, Ordering::SeqCst))),
        ..Default::default()
    };
    claims.extra.insert("htm".to_string(), json!(htm));
    claims.extra.insert("htu".to_string(), json!(htu));
    if let Some(nonce) = nonce {
        claims.extra.insert("nonce".to_string(), json!(nonce));
    }
    if let Some(access_token) = access_token {
        claims.extra.insert(
            "ath".to_string(),
            json!(URL_SAFE_NO_PAD.encode(Sha256::digest(access_token.as_bytes()))),
        );
    }
    rsky_oauth::jwt::sign(&header, &claims, key).unwrap()
}

fn form_encode(pairs: &[(&str, &str)]) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in pairs {
        serializer.append_pair(key, value);
    }
    serializer.finish()
}

fn par_body(client_id: &str, scope: &str, state: &str) -> String {
    form_encode(&[
        ("client_id", client_id),
        ("response_type", "code"),
        ("redirect_uri", REDIRECT_URI),
        ("scope", scope),
        ("state", state),
        ("code_challenge", PKCE_CHALLENGE),
        ("code_challenge_method", "S256"),
    ])
}

/// PAR with the standard `use_dpop_nonce` retry dance; returns the
/// request_uri and the fresh server nonce.
async fn run_par(client: &Client, key: &Jwk) -> (String, String) {
    run_par_scoped(
        client,
        key,
        LOOPBACK_CLIENT_ID,
        "atproto transition:generic",
    )
    .await
}

/// [`run_par`], but for a client_id/scope other than the default loopback
/// client, so a scoped-access-guard test can request a narrow grant (e.g.
/// `atproto blob:image/*`) instead of `transition:generic`.
async fn run_par_scoped(
    client: &Client,
    key: &Jwk,
    client_id: &str,
    scope: &str,
) -> (String, String) {
    let htu = format!("{}/oauth/par", public_url(client));
    let response = client
        .post("/oauth/par")
        .header(ContentType::Form)
        .header(Header::new(
            "DPoP",
            dpop_proof(key, "POST", &htu, None, None),
        ))
        .body(par_body(client_id, scope, "state-123"))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::BadRequest);
    let nonce = response
        .headers()
        .get_one("DPoP-Nonce")
        .expect("DPoP-Nonce header on nonce challenge")
        .to_string();
    let body: Value = serde_json::from_str(&response.into_string().await.unwrap()).unwrap();
    assert_eq!(body["error"], "use_dpop_nonce");

    let response = client
        .post("/oauth/par")
        .header(ContentType::Form)
        .header(Header::new(
            "DPoP",
            dpop_proof(key, "POST", &htu, Some(&nonce), None),
        ))
        .body(par_body(client_id, scope, "state-123"))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().await.unwrap()).unwrap();
    assert_eq!(body["expires_in"], 300);
    let request_uri = body["request_uri"].as_str().unwrap().to_string();
    assert!(request_uri.starts_with("urn:ietf:params:oauth:request_uri:req-"));
    (request_uri, nonce)
}

fn extract_csrf(html: &str) -> String {
    let marker = "name=\"csrf\" value=\"";
    let start = html.find(marker).expect("csrf field in page") + marker.len();
    let end = html[start..].find('"').unwrap() + start;
    html[start..end].to_string()
}

fn authorize_path(client_id: &str, request_uri: &str) -> String {
    format!(
        "/oauth/authorize?{}",
        form_encode(&[("client_id", client_id), ("request_uri", request_uri)])
    )
}

struct AuthorizeSession {
    cookie: String,
    csrf: String,
}

/// GET /oauth/authorize, returning the device cookie and csrf token.
async fn open_authorize_page(client: &Client, request_uri: &str) -> AuthorizeSession {
    open_authorize_page_scoped(client, LOOPBACK_CLIENT_ID, request_uri).await
}

/// [`open_authorize_page`] for a non-default client_id.
async fn open_authorize_page_scoped(
    client: &Client,
    client_id: &str,
    request_uri: &str,
) -> AuthorizeSession {
    let response = client
        .get(authorize_path(client_id, request_uri))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let cookie = response
        .cookies()
        .get("device-id")
        .expect("device cookie set")
        .value()
        .to_string();
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Sign in"));
    assert!(html.contains(request_uri));
    AuthorizeSession {
        cookie,
        csrf: extract_csrf(&html),
    }
}

async fn sign_in_and_accept(
    client: &Client,
    request_uri: &str,
    session: &AuthorizeSession,
) -> String {
    sign_in_and_accept_scoped(client, LOOPBACK_CLIENT_ID, request_uri, session).await
}

/// [`sign_in_and_accept`] for a non-default client_id.
async fn sign_in_and_accept_scoped(
    client: &Client,
    client_id: &str,
    request_uri: &str,
    session: &AuthorizeSession,
) -> String {
    let response = client
        .post("/oauth/authorize/sign-in")
        .header(ContentType::Form)
        .cookie(("device-id", session.cookie.clone()))
        .body(form_encode(&[
            ("request_uri", request_uri),
            ("client_id", client_id),
            ("csrf", &session.csrf),
            ("identifier", "foo@example.com"),
            ("password", "password"),
        ]))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Authorize"));
    assert!(html.contains("Uniquely identify your account"));
    assert!(html.contains("did:plc:khvyd3oiw46vif5gm7hijslk"));

    let response = client
        .post("/oauth/authorize/accept")
        .header(ContentType::Form)
        .cookie(("device-id", session.cookie.clone()))
        .body(form_encode(&[
            ("request_uri", request_uri),
            ("client_id", client_id),
            ("csrf", &session.csrf),
            ("did", "did:plc:khvyd3oiw46vif5gm7hijslk"),
        ]))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::SeeOther);
    let location = response
        .headers()
        .get_one("Location")
        .expect("redirect location")
        .to_string();
    assert!(location.starts_with(REDIRECT_URI));
    assert!(location.contains("state=state-123"));
    assert!(location.contains("iss="));
    let url = url::Url::parse(&location).unwrap();
    url.query_pairs()
        .find(|(key, _)| key == "code")
        .map(|(_, value)| value.into_owned())
        .expect("code in redirect")
}

async fn exchange_code(client: &Client, key: &Jwk, code: &str, nonce: &str) -> Value {
    exchange_code_scoped(client, LOOPBACK_CLIENT_ID, key, code, nonce).await
}

/// [`exchange_code`] for a non-default client_id.
async fn exchange_code_scoped(
    client: &Client,
    client_id: &str,
    key: &Jwk,
    code: &str,
    nonce: &str,
) -> Value {
    let htu = format!("{}/oauth/token", public_url(client));
    let response = client
        .post("/oauth/token")
        .header(ContentType::Form)
        .header(Header::new(
            "DPoP",
            dpop_proof(key, "POST", &htu, Some(nonce), None),
        ))
        .body(form_encode(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", client_id),
            ("redirect_uri", REDIRECT_URI),
            ("code_verifier", PKCE_VERIFIER),
        ]))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    serde_json::from_str(&response.into_string().await.unwrap()).unwrap()
}

async fn activate_test_account(client: &Client) {
    let account_manager = client
        .rocket()
        .state::<rsky_pds::account_manager::AccountManager>()
        .unwrap();
    account_manager
        .activate_account("did:plc:khvyd3oiw46vif5gm7hijslk")
        .await
        .unwrap();
}

#[tokio::test]
async fn oauth_well_known_documents() {
    let (_dir, client) = get_oauth_client().await;
    let issuer = public_url(&client);

    let response = client
        .get("/.well-known/oauth-authorization-server")
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().await.unwrap()).unwrap();
    assert_eq!(body["issuer"], issuer);
    assert_eq!(
        body["pushed_authorization_request_endpoint"],
        format!("{issuer}/oauth/par")
    );
    assert_eq!(body["require_pushed_authorization_requests"], true);
    assert_eq!(body["client_id_metadata_document_supported"], true);
    assert_eq!(body["code_challenge_methods_supported"], json!(["S256"]));

    let response = client
        .get("/.well-known/oauth-protected-resource")
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().await.unwrap()).unwrap();
    assert_eq!(body["resource"], issuer);
    assert_eq!(body["authorization_servers"], json!([issuer]));

    let response = client.get("/oauth/jwks").dispatch().await;
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().await.unwrap()).unwrap();
    assert_eq!(body["keys"].as_array().unwrap().len(), 1);
    assert!(body["keys"][0].get("d").is_none());
    assert_eq!(body["keys"][0]["crv"], "secp256k1");
}

#[tokio::test]
async fn oauth_full_flow_with_dpop_bound_resource_access() {
    let (_dir, client) = get_oauth_client().await;
    common::create_account(&client).await;
    activate_test_account(&client).await;
    let key = dpop_key();

    let (request_uri, nonce) = run_par(&client, &key).await;
    let session = open_authorize_page(&client, &request_uri).await;
    let code = sign_in_and_accept(&client, &request_uri, &session).await;
    let tokens = exchange_code(&client, &key, &code, &nonce).await;
    assert_eq!(tokens["token_type"], "DPoP");
    assert_eq!(tokens["sub"], "did:plc:khvyd3oiw46vif5gm7hijslk");
    assert_eq!(tokens["scope"], "atproto transition:generic");
    let access_token = tokens["access_token"].as_str().unwrap().to_string();
    let refresh_token = tokens["refresh_token"].as_str().unwrap().to_string();

    // resource request without a nonce is challenged and re-tried
    let session_htu = format!("{}/xrpc/com.atproto.server.getSession", public_url(&client));
    let response = client
        .get("/xrpc/com.atproto.server.getSession")
        .header(Header::new("Authorization", format!("DPoP {access_token}")))
        .header(Header::new(
            "DPoP",
            dpop_proof(&key, "GET", &session_htu, None, Some(&access_token)),
        ))
        .dispatch()
        .await;
    // RFC 9449 §8: a missing DPoP nonce is challenged with 401 use_dpop_nonce.
    assert_eq!(response.status(), Status::Unauthorized);
    let resource_nonce = response
        .headers()
        .get_one("DPoP-Nonce")
        .expect("nonce challenge on resource request")
        .to_string();

    let response = client
        .get("/xrpc/com.atproto.server.getSession")
        .header(Header::new("Authorization", format!("DPoP {access_token}")))
        .header(Header::new(
            "DPoP",
            dpop_proof(
                &key,
                "GET",
                &session_htu,
                Some(&resource_nonce),
                Some(&access_token),
            ),
        ))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().await.unwrap()).unwrap();
    assert_eq!(body["did"], "did:plc:khvyd3oiw46vif5gm7hijslk");

    // a proof signed by a different key is rejected
    let wrong_key = Jwk::from_private_key_bytes(EcCurve::P256, &[0x52u8; 32]).unwrap();
    let response = client
        .get("/xrpc/com.atproto.server.getSession")
        .header(Header::new("Authorization", format!("DPoP {access_token}")))
        .header(Header::new(
            "DPoP",
            dpop_proof(
                &wrong_key,
                "GET",
                &session_htu,
                Some(&resource_nonce),
                Some(&access_token),
            ),
        ))
        .dispatch()
        .await;
    // A proof signed by the wrong DPoP key is an authentication failure (401).
    assert_eq!(response.status(), Status::Unauthorized);
    assert!(response
        .headers()
        .get_one("WWW-Authenticate")
        .unwrap()
        .contains("invalid_token"));

    // refresh rotation
    let token_htu = format!("{}/oauth/token", public_url(&client));
    let response = client
        .post("/oauth/token")
        .header(ContentType::Form)
        .header(Header::new(
            "DPoP",
            dpop_proof(&key, "POST", &token_htu, Some(&nonce), None),
        ))
        .body(form_encode(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh_token),
            ("client_id", LOOPBACK_CLIENT_ID),
        ]))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let rotated: Value = serde_json::from_str(&response.into_string().await.unwrap()).unwrap();
    let new_refresh_token = rotated["refresh_token"].as_str().unwrap().to_string();
    assert_ne!(new_refresh_token, refresh_token);

    // replaying the rotated-out refresh token kills the session
    let response = client
        .post("/oauth/token")
        .header(ContentType::Form)
        .header(Header::new(
            "DPoP",
            dpop_proof(&key, "POST", &token_htu, Some(&nonce), None),
        ))
        .body(form_encode(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh_token),
            ("client_id", LOOPBACK_CLIENT_ID),
        ]))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::BadRequest);
    let body: Value = serde_json::from_str(&response.into_string().await.unwrap()).unwrap();
    assert_eq!(body["error"], "invalid_grant");
}

#[tokio::test]
async fn oauth_revocation() {
    let (_dir, client) = get_oauth_client().await;
    common::create_account(&client).await;
    activate_test_account(&client).await;
    let key = dpop_key();

    let (request_uri, nonce) = run_par(&client, &key).await;
    let session = open_authorize_page(&client, &request_uri).await;
    let code = sign_in_and_accept(&client, &request_uri, &session).await;
    let tokens = exchange_code(&client, &key, &code, &nonce).await;
    let access_token = tokens["access_token"].as_str().unwrap().to_string();
    let refresh_token = tokens["refresh_token"].as_str().unwrap().to_string();

    let response = client
        .post("/oauth/revoke")
        .header(ContentType::Form)
        .body(form_encode(&[
            ("token", &refresh_token),
            ("client_id", LOOPBACK_CLIENT_ID),
        ]))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);

    // the access token no longer works
    let session_htu = format!("{}/xrpc/com.atproto.server.getSession", public_url(&client));
    let challenge = client
        .get("/xrpc/com.atproto.server.getSession")
        .header(Header::new("Authorization", format!("DPoP {access_token}")))
        .header(Header::new(
            "DPoP",
            dpop_proof(&key, "GET", &session_htu, None, Some(&access_token)),
        ))
        .dispatch()
        .await;
    let resource_nonce = challenge
        .headers()
        .get_one("DPoP-Nonce")
        .unwrap()
        .to_string();
    let response = client
        .get("/xrpc/com.atproto.server.getSession")
        .header(Header::new("Authorization", format!("DPoP {access_token}")))
        .header(Header::new(
            "DPoP",
            dpop_proof(
                &key,
                "GET",
                &session_htu,
                Some(&resource_nonce),
                Some(&access_token),
            ),
        ))
        .dispatch()
        .await;
    // A revoked token is an authentication failure (401), not a client error.
    assert_eq!(response.status(), Status::Unauthorized);
    assert!(response
        .headers()
        .get_one("WWW-Authenticate")
        .unwrap()
        .contains("revoked"));
}

#[tokio::test]
async fn oauth_authorize_error_pages() {
    let (_dir, client) = get_oauth_client().await;

    let response = client.get("/oauth/authorize").dispatch().await;
    assert_eq!(response.status(), Status::BadRequest);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("client_id and request_uri are required"));

    let response = client
        .get(authorize_path(
            LOOPBACK_CLIENT_ID,
            "urn:ietf:params:oauth:request_uri:req-00000000000000000000000000000000",
        ))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::BadRequest);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("unknown request_uri"));
}

#[tokio::test]
async fn oauth_sign_in_failures() {
    let (_dir, client) = get_oauth_client().await;
    common::create_account(&client).await;
    let key = dpop_key();
    let (request_uri, _) = run_par(&client, &key).await;
    let session = open_authorize_page(&client, &request_uri).await;

    // wrong password re-renders the sign-in page with an error
    let response = client
        .post("/oauth/authorize/sign-in")
        .header(ContentType::Form)
        .cookie(("device-id", session.cookie.clone()))
        .body(form_encode(&[
            ("request_uri", &request_uri),
            ("client_id", LOOPBACK_CLIENT_ID),
            ("csrf", &session.csrf),
            ("identifier", "foo@example.com"),
            ("password", "wrong-password"),
        ]))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("invalid identifier or password"));

    // csrf mismatch is rejected
    let response = client
        .post("/oauth/authorize/sign-in")
        .header(ContentType::Form)
        .cookie(("device-id", session.cookie.clone()))
        .body(form_encode(&[
            ("request_uri", &request_uri),
            ("client_id", LOOPBACK_CLIENT_ID),
            ("csrf", "forged"),
            ("identifier", "foo@example.com"),
            ("password", "password"),
        ]))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::BadRequest);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("invalid CSRF token"));
}

#[tokio::test]
async fn oauth_reject_redirects_with_access_denied() {
    let (_dir, client) = get_oauth_client().await;
    common::create_account(&client).await;
    let key = dpop_key();
    let (request_uri, _) = run_par(&client, &key).await;
    let session = open_authorize_page(&client, &request_uri).await;

    let response = client
        .post("/oauth/authorize/reject")
        .header(ContentType::Form)
        .cookie(("device-id", session.cookie.clone()))
        .body(form_encode(&[
            ("request_uri", &request_uri),
            ("client_id", LOOPBACK_CLIENT_ID),
            ("csrf", &session.csrf),
        ]))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::SeeOther);
    let location = response.headers().get_one("Location").unwrap();
    assert!(location.contains("error=access_denied"));
    assert!(location.contains("state=state-123"));
}

#[tokio::test]
async fn oauth_account_picker_select_flow() {
    let (_dir, client) = get_oauth_client().await;
    common::create_account(&client).await;
    let key = dpop_key();

    // first round signs the device in
    let (request_uri, nonce) = run_par(&client, &key).await;
    let session = open_authorize_page(&client, &request_uri).await;
    let code = sign_in_and_accept(&client, &request_uri, &session).await;
    exchange_code(&client, &key, &code, &nonce).await;

    // second round shows the signed-in account and supports select
    let (request_uri, _) = run_par(&client, &key).await;
    let response = client
        .get(authorize_path(LOOPBACK_CLIENT_ID, &request_uri))
        .cookie(("device-id", session.cookie.clone()))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Continue as"));

    let response = client
        .post("/oauth/authorize/select")
        .header(ContentType::Form)
        .cookie(("device-id", session.cookie.clone()))
        .body(form_encode(&[
            ("request_uri", &request_uri),
            ("client_id", LOOPBACK_CLIENT_ID),
            ("csrf", &session.csrf),
            ("did", "did:plc:khvyd3oiw46vif5gm7hijslk"),
        ]))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Authorize"));

    // selecting an account that never signed in on this device fails
    let response = client
        .post("/oauth/authorize/select")
        .header(ContentType::Form)
        .cookie(("device-id", session.cookie.clone()))
        .body(form_encode(&[
            ("request_uri", &request_uri),
            ("client_id", LOOPBACK_CLIENT_ID),
            ("csrf", &session.csrf),
            ("did", "did:plc:someoneelse"),
        ]))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::BadRequest);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("not signed in on this device"));
}

#[tokio::test]
async fn oauth_device_cookie_with_stale_session_is_replaced() {
    let (_dir, client) = get_oauth_client().await;
    common::create_account(&client).await;
    let key = dpop_key();
    let (request_uri, _) = run_par(&client, &key).await;
    let session = open_authorize_page(&client, &request_uri).await;
    let device_id = session.cookie.split_once('.').unwrap().0.to_string();

    // a cookie naming a real device but a stale session id gets replaced
    let (request_uri, _) = run_par(&client, &key).await;
    let response = client
        .get(authorize_path(LOOPBACK_CLIENT_ID, &request_uri))
        .cookie(("device-id", format!("{device_id}.ses-forged")))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let fresh = response
        .cookies()
        .get("device-id")
        .expect("fresh device cookie issued")
        .value()
        .to_string();
    assert_ne!(fresh, format!("{device_id}.ses-forged"));

    // malformed cookies (no separator) are also replaced
    let (request_uri, _) = run_par(&client, &key).await;
    let response = client
        .get(authorize_path(LOOPBACK_CLIENT_ID, &request_uri))
        .cookie(("device-id", "garbage-cookie"))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    assert!(response.cookies().get("device-id").is_some());
}

#[tokio::test]
async fn oauth_endpoint_edge_cases() {
    let (_dir, client) = get_oauth_client().await;

    // revoke without a token parameter
    let response = client
        .post("/oauth/revoke")
        .header(ContentType::Form)
        .body(form_encode(&[("client_id", LOOPBACK_CLIENT_ID)]))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::BadRequest);
    let body: Value = serde_json::from_str(&response.into_string().await.unwrap()).unwrap();
    assert_eq!(body["error"], "invalid_request");

    // token endpoint with an unknown client
    let htu = format!("{}/oauth/token", public_url(&client));
    let key = dpop_key();
    let response = client
        .post("/oauth/token")
        .header(ContentType::Form)
        .header(Header::new(
            "DPoP",
            dpop_proof(&key, "POST", &htu, None, None),
        ))
        .body(form_encode(&[
            ("grant_type", "authorization_code"),
            ("client_id", "https://unknown.example.com/client.json"),
            ("code", "cod-x"),
        ]))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Unauthorized);

    // accept without a did field
    let response = client
        .post("/oauth/authorize/accept")
        .header(ContentType::Form)
        .body(form_encode(&[
            ("request_uri", "urn:x"),
            ("client_id", LOOPBACK_CLIENT_ID),
            ("csrf", "whatever"),
        ]))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::BadRequest);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("invalid CSRF token"));
}

// Scoped-access guard tests
// ---------------------
//
// These prove the combinator guards in `auth_verifier.rs`
// (`BlobScopedAccess`, `HandleScopedAccess`, `EmailScopedAccess`,
// `AccountStatusScopedAccess`, `AccountRepoScopedAccess`) enforce their
// resource scope through a real dispatched request over the full Rocket
// stack -- DPoP verification, session lookup, and all -- rather than just
// exercising the `assert_*_scope` helper in isolation. That is the point of
// folding the assertion into the guard: a route that takes one of these
// guards cannot get a `did` without the check having already run, so there
// is no separate call in the handler body left to accidentally skip.
//
// `BlobScopedAccess` and `HandleScopedAccess` wrap `AccessStandardCheckTakedown`
// and `AccountRepoScopedAccess<AccessStandard>` wraps `AccessStandard`, both
// of which accept an OAuth session mapped to `AppPass`/`AppPassPrivileged` --
// exactly what a granular-scope grant maps to (see
// `auth_verifier::oauth_scopes_to_auth_scope`), so a real PAR/authorize/token
// flow can reach them. `EmailScopedAccess`, `AccountStatusScopedAccess`, and
// the default (`AccessFull`) `AccountRepoScopedAccess` wrap `AccessFull`,
// which requires `AuthScope::Access` -- a scope only the legacy plain-JWT
// `createSession` token type carries, and that token type always sets
// `granted_scopes: None`. No OAuth grant can ever produce `AuthScope::Access`
// (`oauth_scopes_to_auth_scope` only ever returns `AppPass`/`AppPassPrivileged`
// or rejects), so those three guards' scope check is presently unreachable
// through any live request; per-endpoint test coverage for them stays at the
// `apis::tests` level added alongside `assert_account_scope` itself, which
// exercises the identical helper this guard calls.

/// Runs the full PAR -> authorize -> sign-in -> accept -> token-exchange
/// dance for a fresh loopback client requesting exactly `scope`, returning a
/// real DPoP-bound access token and the key it is bound to.
async fn granular_access_token(client: &Client, scope: &str) -> (String, Jwk) {
    let key = dpop_key();
    let client_id = loopback_client_id(scope);
    let (request_uri, nonce) = run_par_scoped(client, &key, &client_id, scope).await;
    let session = open_authorize_page_scoped(client, &client_id, &request_uri).await;
    let code = sign_in_and_accept_scoped(client, &client_id, &request_uri, &session).await;
    let tokens = exchange_code_scoped(client, &client_id, &key, &code, &nonce).await;
    (tokens["access_token"].as_str().unwrap().to_string(), key)
}

/// Dispatches an authenticated POST, retrying once with the server's DPoP
/// nonce the same way a real client would (RFC 9449 §8), and returns the
/// final status and parsed JSON body.
async fn dispatch_scoped_post(
    client: &Client,
    path: &str,
    access_token: &str,
    key: &Jwk,
    content_type: ContentType,
    body: Vec<u8>,
) -> (Status, Value) {
    let htu = format!("{}{}", public_url(client), path);
    let first = client
        .post(path)
        .header(content_type.clone())
        .header(Header::new("Authorization", format!("DPoP {access_token}")))
        .header(Header::new(
            "DPoP",
            dpop_proof(key, "POST", &htu, None, Some(access_token)),
        ))
        .body(body.clone())
        .dispatch()
        .await;
    let response = if first.status() == Status::Unauthorized {
        match first.headers().get_one("DPoP-Nonce").map(str::to_string) {
            Some(nonce) => {
                client
                    .post(path)
                    .header(content_type)
                    .header(Header::new("Authorization", format!("DPoP {access_token}")))
                    .header(Header::new(
                        "DPoP",
                        dpop_proof(key, "POST", &htu, Some(&nonce), Some(access_token)),
                    ))
                    .body(body)
                    .dispatch()
                    .await
            }
            None => first,
        }
    } else {
        first
    };
    let status = response.status();
    let text = response.into_string().await.unwrap_or_default();
    let json_body = serde_json::from_str(&text).unwrap_or(Value::Null);
    (status, json_body)
}

fn handle_domain(client: &Client) -> String {
    client
        .rocket()
        .state::<rsky_pds::config::ServerConfig>()
        .unwrap()
        .identity
        .service_handle_domains
        .first()
        .unwrap()
        .clone()
}

#[tokio::test]
async fn blob_scoped_access_allows_matching_mime_and_denies_mismatch() {
    let (_dir, client) = get_oauth_client().await;
    common::create_account(&client).await;
    activate_test_account(&client).await;

    // A `blob:image/*` grant permits uploading a png.
    let (access_token, key) = granular_access_token(&client, "atproto blob:image/*").await;
    let (status, body) = dispatch_scoped_post(
        &client,
        "/xrpc/com.atproto.repo.uploadBlob",
        &access_token,
        &key,
        ContentType::PNG,
        vec![0x89, 0x50, 0x4e, 0x47],
    )
    .await;
    assert_eq!(status, Status::Ok, "body was {body}");
    assert!(
        body["blob"]["mimeType"].as_str().is_some(),
        "body was {body}"
    );

    // The same grant does not cover a mismatched mime type.
    let (access_token, key) = granular_access_token(&client, "atproto blob:image/*").await;
    let (status, body) = dispatch_scoped_post(
        &client,
        "/xrpc/com.atproto.repo.uploadBlob",
        &access_token,
        &key,
        ContentType::Plain,
        b"not an image".to_vec(),
    )
    .await;
    assert_eq!(status, Status::Forbidden, "body was {body}");
    assert_eq!(body["error"], "InsufficientScope", "body was {body}");
}

#[tokio::test]
async fn handle_scoped_access_allows_handle_and_denies_other_attr() {
    let (_dir, client) = get_oauth_client().await;
    common::create_account(&client).await;
    activate_test_account(&client).await;
    let domain = handle_domain(&client);

    // An `identity:handle` grant clears `HandleScopedAccess` and reaches the
    // handler, which then talks to the mock PLC directory to rotate the
    // did:plc handle -- that mock only serves DID documents, not the PLC
    // operation-log shape `plc::Client::update_handle` needs, so the request
    // still fails past the guard. What matters here is that it is NOT
    // rejected by the scope check.
    let (access_token, key) = granular_access_token(&client, "atproto identity:handle").await;
    let (status, body) = dispatch_scoped_post(
        &client,
        "/xrpc/com.atproto.identity.updateHandle",
        &access_token,
        &key,
        ContentType::JSON,
        json!({ "handle": format!("bar{domain}") })
            .to_string()
            .into_bytes(),
    )
    .await;
    assert_ne!(status, Status::Forbidden, "body was {body}");
    assert_ne!(body["error"], "InsufficientScope", "body was {body}");

    // An `identity:` grant for an attribute this server does not recognise
    // (this parser only accepts `handle`/`*`) is still a granular identity
    // session -- it engages restriction -- but permits nothing.
    let (access_token, key) = granular_access_token(&client, "atproto identity:invalid").await;
    let (status, body) = dispatch_scoped_post(
        &client,
        "/xrpc/com.atproto.identity.updateHandle",
        &access_token,
        &key,
        ContentType::JSON,
        json!({ "handle": format!("baz{domain}") })
            .to_string()
            .into_bytes(),
    )
    .await;
    assert_eq!(status, Status::Forbidden, "body was {body}");
    assert_eq!(body["error"], "InsufficientScope", "body was {body}");
}

#[tokio::test]
async fn account_repo_scoped_access_covers_submit_plc_operation() {
    let (_dir, client) = get_oauth_client().await;
    common::create_account(&client).await;
    activate_test_account(&client).await;

    // `account:repo?action=manage` clears `AccountRepoScopedAccess<AccessStandard>`
    // and reaches the handler's own request-body validation, which rejects
    // this empty operation -- proving the guard let the request through
    // rather than stopping it at the scope check.
    let (access_token, key) =
        granular_access_token(&client, "atproto account:repo?action=manage").await;
    let (status, body) = dispatch_scoped_post(
        &client,
        "/xrpc/com.atproto.identity.submitPlcOperation",
        &access_token,
        &key,
        ContentType::JSON,
        json!({ "operation": {} }).to_string().into_bytes(),
    )
    .await;
    assert_ne!(body["error"], "InsufficientScope", "body was {body}");
    assert_eq!(status, Status::BadRequest, "body was {body}");

    // `account:repo` alone defaults to `action=read`, which does not satisfy
    // the `Manage` check `submitPlcOperation` requires.
    let (access_token, key) = granular_access_token(&client, "atproto account:repo").await;
    let (status, body) = dispatch_scoped_post(
        &client,
        "/xrpc/com.atproto.identity.submitPlcOperation",
        &access_token,
        &key,
        ContentType::JSON,
        json!({ "operation": {} }).to_string().into_bytes(),
    )
    .await;
    assert_eq!(status, Status::Forbidden, "body was {body}");
    assert_eq!(body["error"], "InsufficientScope", "body was {body}");
}

/// `EmailScopedAccess`, `AccountStatusScopedAccess`, and the default
/// `AccountRepoScopedAccess` all wrap `AccessFull`, which requires
/// `AuthScope::Access` -- a scope only a legacy plain-JWT `createSession`
/// token carries (see the module doc above). This is a regression check that
/// the refactor did not break that, the only path currently able to reach
/// them: a legacy session still clears `EmailScopedAccess` exactly as it
/// cleared the plain `AccessFull` guard before this change.
#[tokio::test]
async fn email_scoped_access_still_accepts_a_legacy_session() {
    let (_dir, client) = get_oauth_client().await;
    let (identifier, password) = common::create_account(&client).await;
    let response = client
        .post("/xrpc/com.atproto.server.createSession")
        .header(ContentType::JSON)
        .body(json!({ "identifier": identifier, "password": password }).to_string())
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let session: Value = response.into_json().await.expect("session json");
    let access_jwt = session["accessJwt"]
        .as_str()
        .expect("accessJwt")
        .to_string();

    let response = client
        .post("/xrpc/com.atproto.server.updateEmail")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", format!("Bearer {access_jwt}")))
        .body(json!({ "email": "new-address@example.com" }).to_string())
        .dispatch()
        .await;
    let status = response.status();
    let text = response.into_string().await.unwrap_or_default();
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    assert_eq!(status, Status::Ok, "body was {body}");
    assert_ne!(body["error"], "InsufficientScope", "body was {body}");
}
