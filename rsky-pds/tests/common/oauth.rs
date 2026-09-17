//! Helpers for driving the OAuth endpoints and pages through the local
//! client: DPoP proofs, PAR, the authorization pages and their forms.
#![allow(dead_code)]

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

pub const LOOPBACK_CLIENT_ID: &str =
    "http://localhost?scope=atproto%20transition%3Ageneric&redirect_uri=http%3A%2F%2F127.0.0.1%3A8080%2Fcb";
pub const REDIRECT_URI: &str = "http://127.0.0.1:8080/cb";

/// A loopback client_id requesting exactly `scope` -- the loopback client's
/// metadata is derived from its own URL, so `allowed_scopes` is exactly what
/// this embeds and the PAR request below must ask for the same string.
#[allow(dead_code)] // only the scoped-access-guard tests drive non-default scopes
pub fn loopback_client_id(scope: &str) -> String {
    format!(
        "http://localhost?scope={}&redirect_uri={}",
        url::form_urlencoded::byte_serialize(scope.as_bytes()).collect::<String>(),
        url::form_urlencoded::byte_serialize(REDIRECT_URI.as_bytes()).collect::<String>(),
    )
}
pub const PKCE_VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
pub const PKCE_CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

pub static JTI: AtomicU64 = AtomicU64::new(0);
pub static OAUTH_ENV: std::sync::Once = std::sync::Once::new();

/// Pin the DPoP nonce secret before any provider is constructed so the
/// shared-secret configuration path is exercised.
pub async fn get_oauth_client() -> (tempfile::TempDir, Client) {
    OAUTH_ENV.call_once(|| {
        std::env::set_var(
            "PDS_DPOP_SECRET",
            "0101010101010101010101010101010101010101010101010101010101010101",
        );
    });
    super::get_client().await
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

pub fn public_url(client: &Client) -> String {
    client
        .rocket()
        .state::<rsky_pds::config::ServerConfig>()
        .unwrap()
        .service
        .public_url
        .clone()
}

pub fn dpop_key() -> Jwk {
    Jwk::from_private_key_bytes(EcCurve::P256, &[0x51u8; 32]).unwrap()
}

pub fn dpop_proof(
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

pub fn form_encode(pairs: &[(&str, &str)]) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in pairs {
        serializer.append_pair(key, value);
    }
    serializer.finish()
}

pub fn par_body(client_id: &str, scope: &str, state: &str) -> String {
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
pub async fn run_par(client: &Client, key: &Jwk) -> (String, String) {
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
pub async fn run_par_scoped(
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

pub fn extract_csrf(html: &str) -> String {
    let marker = "name=\"csrf\" value=\"";
    let start = html.find(marker).expect("csrf field in page") + marker.len();
    let end = html[start..].find('"').unwrap() + start;
    html[start..end].to_string()
}

pub fn authorize_path(client_id: &str, request_uri: &str) -> String {
    format!(
        "/oauth/authorize?{}",
        form_encode(&[("client_id", client_id), ("request_uri", request_uri)])
    )
}

pub struct AuthorizeSession {
    pub cookie: String,
    pub csrf: String,
}

/// GET /oauth/authorize, returning the device cookie and csrf token.
pub async fn open_authorize_page(client: &Client, request_uri: &str) -> AuthorizeSession {
    open_authorize_page_scoped(client, LOOPBACK_CLIENT_ID, request_uri).await
}

/// [`open_authorize_page`] for a non-default client_id.
pub async fn open_authorize_page_scoped(
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
    assert!(!html.contains("name=\"email_otp\""));
    AuthorizeSession {
        cookie,
        csrf: extract_csrf(&html),
    }
}

pub async fn sign_in_and_accept(
    client: &Client,
    request_uri: &str,
    session: &mut AuthorizeSession,
) -> String {
    sign_in_and_accept_scoped(client, LOOPBACK_CLIENT_ID, request_uri, session).await
}

/// [`sign_in_and_accept`] for a non-default client_id. A sign-in rotates the
/// device secret, so the session is updated with the cookie and csrf the
/// consent page carries.
pub async fn sign_in_and_accept_scoped(
    client: &Client,
    client_id: &str,
    request_uri: &str,
    session: &mut AuthorizeSession,
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
            ("remember", "on"),
        ]))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let rotated = response
        .cookies()
        .get("device-id")
        .expect("sign-in rotates the device cookie")
        .value()
        .to_string();
    let device_id = session.cookie.split_once('.').unwrap().0.to_string();
    assert!(rotated.starts_with(&format!("{device_id}.")));
    assert_ne!(rotated, session.cookie);
    let html = response.into_string().await.unwrap();
    assert!(!html.contains("session_token"));
    assert!(html.contains("Authorize"));
    assert!(html.contains("Confirm your identity"));
    assert!(html.contains("did:plc:khvyd3oiw46vif5gm7hijslk"));
    session.cookie = rotated;
    session.csrf = extract_csrf(&html);

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

pub async fn exchange_code(client: &Client, key: &Jwk, code: &str, nonce: &str) -> Value {
    exchange_code_scoped(client, LOOPBACK_CLIENT_ID, key, code, nonce).await
}

/// [`exchange_code`] for a non-default client_id.
pub async fn exchange_code_scoped(
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

/// The test account as a browser flow finds it: created and active, so
/// the authorization pages go to consent rather than to reactivation.
pub async fn create_active_account(client: &Client) {
    super::create_account(client).await;
    activate_test_account(client).await;
}

pub async fn activate_test_account(client: &Client) {
    let account_manager = client
        .rocket()
        .state::<rsky_pds::account_manager::AccountManager>()
        .unwrap();
    account_manager
        .activate_account("did:plc:khvyd3oiw46vif5gm7hijslk")
        .await
        .unwrap();
}

/// Fetches a DPoP-bound resource, taking the server's nonce challenge on the
/// first attempt and retrying with it.
pub async fn dpop_get(
    client: &Client,
    key: &Jwk,
    access_token: &str,
    path: &str,
) -> (Status, Value) {
    let htu = format!("{}{}", public_url(client), path.split('?').next().unwrap());
    let response = client
        .get(path)
        .header(Header::new("Authorization", format!("DPoP {access_token}")))
        .header(Header::new(
            "DPoP",
            dpop_proof(key, "GET", &htu, None, Some(access_token)),
        ))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Unauthorized);
    let nonce = response
        .headers()
        .get_one("DPoP-Nonce")
        .expect("nonce challenge on resource request")
        .to_string();
    let response = client
        .get(path)
        .header(Header::new("Authorization", format!("DPoP {access_token}")))
        .header(Header::new(
            "DPoP",
            dpop_proof(key, "GET", &htu, Some(&nonce), Some(access_token)),
        ))
        .dispatch()
        .await;
    let status = response.status();
    let body: Value = serde_json::from_str(&response.into_string().await.unwrap()).unwrap();
    (status, body)
}

pub async fn dpop_post(
    client: &Client,
    key: &Jwk,
    access_token: &str,
    path: &str,
    body: Value,
) -> (Status, Value) {
    let htu = format!("{}{}", public_url(client), path);
    let response = client
        .post(path)
        .header(ContentType::JSON)
        .header(Header::new("Authorization", format!("DPoP {access_token}")))
        .header(Header::new(
            "DPoP",
            dpop_proof(key, "POST", &htu, None, Some(access_token)),
        ))
        .body(body.to_string())
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Unauthorized);
    let nonce = response
        .headers()
        .get_one("DPoP-Nonce")
        .expect("nonce challenge on resource request")
        .to_string();
    let response = client
        .post(path)
        .header(ContentType::JSON)
        .header(Header::new("Authorization", format!("DPoP {access_token}")))
        .header(Header::new(
            "DPoP",
            dpop_proof(key, "POST", &htu, Some(&nonce), Some(access_token)),
        ))
        .body(body.to_string())
        .dispatch()
        .await;
    let status = response.status();
    let body: Value = serde_json::from_str(&response.into_string().await.unwrap()).unwrap();
    (status, body)
}

pub async fn generic_oauth_access_token(client: &Client, key: &Jwk) -> String {
    let (request_uri, nonce) = run_par(client, key).await;
    let mut session = open_authorize_page(client, &request_uri).await;
    let code = sign_in_and_accept(client, &request_uri, &mut session).await;
    let tokens = exchange_code(client, key, &code, &nonce).await;
    tokens["access_token"].as_str().unwrap().to_string()
}

/// PAR with extra request parameters (a prompt, a login hint), after the
/// nonce challenge. Returns the request_uri and the fresh server nonce.
pub async fn run_par_with(
    client: &Client,
    key: &Jwk,
    client_id: &str,
    scope: &str,
    extra: &[(&str, &str)],
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
    let nonce = response
        .headers()
        .get_one("DPoP-Nonce")
        .expect("DPoP-Nonce header on nonce challenge")
        .to_string();
    let mut pairs = vec![
        ("client_id", client_id),
        ("response_type", "code"),
        ("redirect_uri", REDIRECT_URI),
        ("scope", scope),
        ("state", "state-123"),
        ("code_challenge", PKCE_CHALLENGE),
        ("code_challenge_method", "S256"),
    ];
    pairs.extend_from_slice(extra);
    let response = client
        .post("/oauth/par")
        .header(ContentType::Form)
        .header(Header::new(
            "DPoP",
            dpop_proof(key, "POST", &htu, Some(&nonce), None),
        ))
        .body(form_encode(&pairs))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().await.unwrap()).unwrap();
    (body["request_uri"].as_str().unwrap().to_string(), nonce)
}

pub async fn get_authorize_html(
    client: &Client,
    path: String,
    session: &AuthorizeSession,
) -> String {
    let response = client
        .get(path)
        .cookie(("device-id", session.cookie.clone()))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    response.into_string().await.unwrap()
}
