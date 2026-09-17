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
    assert!(!html.contains("name=\"email_otp\""));
    AuthorizeSession {
        cookie,
        csrf: extract_csrf(&html),
    }
}

async fn sign_in_and_accept(
    client: &Client,
    request_uri: &str,
    session: &mut AuthorizeSession,
) -> String {
    sign_in_and_accept_scoped(client, LOOPBACK_CLIENT_ID, request_uri, session).await
}

/// [`sign_in_and_accept`] for a non-default client_id. A sign-in rotates the
/// device secret, so the session is updated with the cookie and csrf the
/// consent page carries.
async fn sign_in_and_accept_scoped(
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

/// Fetches a DPoP-bound resource, taking the server's nonce challenge on the
/// first attempt and retrying with it.
async fn dpop_get(client: &Client, key: &Jwk, access_token: &str, path: &str) -> (Status, Value) {
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

async fn dpop_post(
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

async fn generic_oauth_access_token(client: &Client, key: &Jwk) -> String {
    let (request_uri, nonce) = run_par(client, key).await;
    let mut session = open_authorize_page(client, &request_uri).await;
    let code = sign_in_and_accept(client, &request_uri, &mut session).await;
    let tokens = exchange_code(client, key, &code, &nonce).await;
    tokens["access_token"].as_str().unwrap().to_string()
}

/// The reference admits OAuth sessions to the full-access methods and lets
/// each route's permission check decide: `checkAccountStatus` always allows,
/// credential management always refuses, and the PLC signing surface needs
/// the identity grant a `transition:generic` session does not carry.
#[tokio::test]
async fn oauth_session_reaches_full_access_routes_per_their_permission_checks() {
    let (_dir, client) = get_oauth_client().await;
    common::create_account(&client).await;
    activate_test_account(&client).await;
    let key = dpop_key();
    let access_token = generic_oauth_access_token(&client, &key).await;

    let (status, body) = dpop_get(
        &client,
        &key,
        &access_token,
        "/xrpc/com.atproto.server.checkAccountStatus",
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
    assert_eq!(body["activated"], true);

    let (status, body) = dpop_get(
        &client,
        &key,
        &access_token,
        "/xrpc/com.atproto.server.listAppPasswords",
    )
    .await;
    assert_eq!(status, Status::Forbidden, "{body}");
    assert_eq!(
        body["message"],
        "OAuth credentials are not supported for this endpoint"
    );

    let (status, body) = dpop_post(
        &client,
        &key,
        &access_token,
        "/xrpc/com.atproto.server.deactivateAccount",
        json!({}),
    )
    .await;
    assert_eq!(status, Status::Forbidden, "{body}");

    let (status, body) = dpop_post(
        &client,
        &key,
        &access_token,
        "/xrpc/com.atproto.identity.requestPlcOperationSignature",
        json!({}),
    )
    .await;
    assert_ne!(status, Status::Ok, "{body}");
    assert_eq!(body["error"], "InsufficientScope", "{body}");
}

/// An OAuth session with `transition:generic` can mint a service token for
/// a non-privileged method, the way an app password can, but not for the
/// chat surface it was never granted.
#[tokio::test]
async fn oauth_session_can_mint_service_auth_tokens() {
    let (_dir, client) = get_oauth_client().await;
    common::create_account(&client).await;
    activate_test_account(&client).await;
    let key = dpop_key();

    let (request_uri, nonce) = run_par(&client, &key).await;
    let mut session = open_authorize_page(&client, &request_uri).await;
    let code = sign_in_and_accept(&client, &request_uri, &mut session).await;
    let tokens = exchange_code(&client, &key, &code, &nonce).await;
    let access_token = tokens["access_token"].as_str().unwrap().to_string();

    let (status, body) = dpop_get(
        &client,
        &key,
        &access_token,
        "/xrpc/com.atproto.server.getServiceAuth\
         ?aud=did:web:video.invalid&lxm=app.bsky.video.getUploadLimits",
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
    assert!(body["token"].as_str().is_some());

    let (status, _) = dpop_get(
        &client,
        &key,
        &access_token,
        "/xrpc/com.atproto.server.getServiceAuth\
         ?aud=did:web:chat.invalid&lxm=chat.bsky.convo.getMessages",
    )
    .await;
    assert_ne!(status, Status::Ok);
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
    let mut session = open_authorize_page(&client, &request_uri).await;
    let code = sign_in_and_accept(&client, &request_uri, &mut session).await;
    let tokens = exchange_code(&client, &key, &code, &nonce).await;
    assert_eq!(tokens["token_type"], "DPoP");
    assert_eq!(tokens["sub"], "did:plc:khvyd3oiw46vif5gm7hijslk");
    assert_eq!(tokens["scope"], "atproto transition:generic");
    let access_token = tokens["access_token"].as_str().unwrap().to_string();
    let refresh_token = tokens["refresh_token"].as_str().unwrap().to_string();

    // The initial authorization_code exchange is both a granted authorization
    // and a new session. The Prometheus recorder is one process-wide
    // instance shared by every test in this binary, so only presence is
    // checked here (not an exact count) -- other tests running concurrently
    // touch these same counters. The authorization_code-vs-refresh_token
    // distinction itself is unit-tested directly in
    // `oauth::routes::tests::is_new_oauth_session_only_true_for_authorization_code`.
    let metrics_body = client
        .get("/metrics")
        .dispatch()
        .await
        .into_string()
        .await
        .unwrap();
    assert!(
        metrics_body.contains("pds_oauth_authorization_grants_total"),
        "{metrics_body}"
    );
    assert!(
        metrics_body.contains(r#"source="oauth"}"#),
        "{metrics_body}"
    );

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
    // Whether this refresh_token grant (or any other) bumps
    // pds_session_created_total is proven by
    // `oauth::routes::tests::is_new_oauth_session_only_true_for_authorization_code`,
    // not asserted here: the Prometheus recorder is one process-wide
    // instance shared by every test in this binary, so an exact count isn't
    // safe to assert on under concurrent test execution.
}

#[tokio::test]
async fn oauth_revocation() {
    let (_dir, client) = get_oauth_client().await;
    common::create_account(&client).await;
    activate_test_account(&client).await;
    let key = dpop_key();

    let (request_uri, nonce) = run_par(&client, &key).await;
    let mut session = open_authorize_page(&client, &request_uri).await;
    let code = sign_in_and_accept(&client, &request_uri, &mut session).await;
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
        .contains("error_description=\"Invalid token\""));
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
    assert!(html.contains("Invalid identifier or password"));

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
    let mut session = open_authorize_page(&client, &request_uri).await;
    let code = sign_in_and_accept(&client, &request_uri, &mut session).await;
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
    assert!(html.contains("Sign in as"));

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

/// A trusted client keeps the prompt it asked for, so once the account has
/// consented, `prompt=none` answers with a code instead of a page.
#[tokio::test]
async fn oauth_trusted_client_gets_silent_consent_after_a_grant() {
    let scope = "atproto transition:email";
    let client_id = loopback_client_id(scope);
    std::env::set_var("PDS_OAUTH_TRUSTED_CLIENTS", &client_id);
    let (_dir, client) = get_oauth_client().await;
    common::create_account(&client).await;
    // a deactivated account never qualifies for single sign-on
    activate_test_account(&client).await;
    let key = dpop_key();

    // first round: the consent screen, and the grant is recorded
    let (request_uri, nonce) = run_par_scoped(&client, &key, &client_id, scope).await;
    let mut session = open_authorize_page_scoped(&client, &client_id, &request_uri).await;
    let code = sign_in_and_accept_scoped(&client, &client_id, &request_uri, &mut session).await;
    exchange_code_scoped(&client, &client_id, &key, &code, &nonce).await;

    // second round: prompt=none with the account named goes straight back
    // to the client
    let htu = format!("{}/oauth/par", public_url(&client));
    let response = client
        .post("/oauth/par")
        .header(ContentType::Form)
        .header(Header::new(
            "DPoP",
            dpop_proof(&key, "POST", &htu, Some(&nonce), None),
        ))
        .body(form_encode(&[
            ("client_id", &client_id),
            ("response_type", "code"),
            ("redirect_uri", REDIRECT_URI),
            ("scope", scope),
            ("state", "state-456"),
            ("code_challenge", PKCE_CHALLENGE),
            ("code_challenge_method", "S256"),
            ("prompt", "none"),
            ("login_hint", "did:plc:khvyd3oiw46vif5gm7hijslk"),
        ]))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().await.unwrap()).unwrap();
    let request_uri = body["request_uri"].as_str().unwrap().to_string();
    let response = client
        .get(authorize_path(&client_id, &request_uri))
        .cookie(("device-id", session.cookie.clone()))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::SeeOther);
    let location = response.headers().get_one("Location").unwrap().to_string();
    assert!(location.starts_with(REDIRECT_URI), "{location}");
    assert!(location.contains("code=cod-"), "{location}");
    assert!(location.contains("state=state-456"), "{location}");
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

// Scope-declaration seam tests
// ---------------------------
//
// These prove that the declaration a route names in its signature
// (`Scoped<Decl, Tier>`, see `auth_verifier::scope`) is enforced through a
// real dispatched request over the full Rocket stack -- DPoP verification,
// session lookup, and all -- rather than just exercising the `assert_*_scope`
// helper in isolation. That is the point of the seam: a route cannot get a
// `did` without the check having already run, so there is no separate call in
// the handler body left to accidentally skip.
//
// Which declarations a real OAuth flow can reach is decided by the tier the
// route pairs them with. `AccessStandard` and `AccessStandardCheckTakedown`
// accept an OAuth session mapped to `AppPass`/`AppPassPrivileged` -- exactly
// what a granular-scope grant maps to (see
// `auth_verifier::oauth_scopes_to_auth_scope`) -- so `BlobUpload`,
// `IdentityHandle`, `RepoWrite`, `RpcProxy`, and the `AccessStandard*`
// pairings of `IdentityFull` and `AccountEmail` are reachable here.
// `AccessFull` requires `AuthScope::Access`, a scope only the legacy plain-JWT
// `createSession` token type carries, and that token type always sets
// `granted_scopes: None`. No OAuth grant can ever produce `AuthScope::Access`,
// so `AccountStatus`, `AccountRepo`, `OAuthForbidden` and the `AccessFull`
// pairings of `AccountEmail` and `IdentityFull` are presently unreachable
// through any live OAuth request; their coverage is the declaration-level
// unit tests in `auth_verifier::scope::tests`, plus the legacy-session
// regression checks at the end of this file.

/// Runs the full PAR -> authorize -> sign-in -> accept -> token-exchange
/// dance for a fresh loopback client requesting exactly `scope`, returning a
/// real DPoP-bound access token and the key it is bound to.
async fn granular_access_token(client: &Client, scope: &str) -> (String, Jwk) {
    let key = dpop_key();
    let client_id = loopback_client_id(scope);
    let (request_uri, nonce) = run_par_scoped(client, &key, &client_id, scope).await;
    let mut session = open_authorize_page_scoped(client, &client_id, &request_uri).await;
    let code = sign_in_and_accept_scoped(client, &client_id, &request_uri, &mut session).await;
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
async fn blob_upload_declaration_allows_matching_mime_and_denies_mismatch() {
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
async fn identity_handle_declaration_allows_handle_and_denies_other_attr() {
    let (_dir, client) = get_oauth_client().await;
    common::create_account(&client).await;
    activate_test_account(&client).await;
    let domain = handle_domain(&client);

    // An `identity:handle` grant clears the `IdentityHandle` declaration and reaches the
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
    // (this parser only accepts `handle`/`*`) permits nothing.
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

/// The defect these guards were carrying: enforcement used to engage only
/// when the session already held a grant of the resource's own kind, so a
/// granular session scoped to one collection and nothing else got
/// *unrestricted* access to every other resource. Absence of a grant is now a
/// denial, proven here over the full stack.
#[tokio::test]
async fn a_repo_only_grant_is_denied_the_resources_it_does_not_name() {
    let (_dir, client) = get_oauth_client().await;
    common::create_account(&client).await;
    activate_test_account(&client).await;
    let domain = handle_domain(&client);
    const REPO_ONLY: &str = "atproto repo:app.bsky.feed.post";

    // No `blob:` grant: the upload is refused rather than waved through.
    let (access_token, key) = granular_access_token(&client, REPO_ONLY).await;
    let (status, body) = dispatch_scoped_post(
        &client,
        "/xrpc/com.atproto.repo.uploadBlob",
        &access_token,
        &key,
        ContentType::PNG,
        vec![0x89, 0x50, 0x4e, 0x47],
    )
    .await;
    assert_eq!(status, Status::Forbidden, "body was {body}");
    assert_eq!(body["error"], "InsufficientScope", "body was {body}");

    // No `identity:` grant: the handle change is refused.
    let (access_token, key) = granular_access_token(&client, REPO_ONLY).await;
    let (status, body) = dispatch_scoped_post(
        &client,
        "/xrpc/com.atproto.identity.updateHandle",
        &access_token,
        &key,
        ContentType::JSON,
        json!({ "handle": format!("qux{domain}") })
            .to_string()
            .into_bytes(),
    )
    .await;
    assert_eq!(status, Status::Forbidden, "body was {body}");
    assert_eq!(body["error"], "InsufficientScope", "body was {body}");

    // No `account:` grant: the PLC submission is refused.
    let (access_token, key) = granular_access_token(&client, REPO_ONLY).await;
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

/// The companion fail-open on the same path: `assert_rpc_scope` used to
/// return `Ok(())` when the destination service could not be resolved, so an
/// audience-resolution error allowed the proxied call outright. An `rpc:`
/// grant is bound to an audience, so an unresolvable one has to deny.
///
/// This mock appview is on loopback, which `is_safe_url` refuses outside dev
/// mode -- so the aud never resolves here, and the request is refused with
/// that failure rather than proxied. Before the fix a repo-only session got
/// through this seam twice over: once for holding no `rpc:` grant, and again
/// for the resolution error.
#[tokio::test]
async fn an_unresolvable_proxy_audience_denies_the_proxied_call() {
    let (_dir, client) = get_oauth_client().await;
    common::create_account(&client).await;
    activate_test_account(&client).await;

    let (access_token, key) =
        granular_access_token(&client, "atproto repo:app.bsky.feed.post").await;
    let (status, body) = dispatch_scoped_post(
        &client,
        "/xrpc/app.bsky.graph.muteActor",
        &access_token,
        &key,
        ContentType::JSON,
        json!({ "actor": "did:plc:someoneelse" })
            .to_string()
            .into_bytes(),
    )
    .await;
    assert_eq!(status, Status::BadRequest, "body was {body}");
    assert_eq!(body["error"], "InvalidRequest", "body was {body}");
}

#[tokio::test]
async fn identity_full_declaration_covers_submit_plc_operation() {
    let (_dir, client) = get_oauth_client().await;
    common::create_account(&client).await;
    activate_test_account(&client).await;

    // `identity:*` clears `Scoped<IdentityFull, AccessStandard>` and reaches
    // the handler's own request-body validation, which rejects this empty
    // operation -- proving the guard let the request through rather than
    // stopping it at the scope check.
    let (access_token, key) = granular_access_token(&client, "atproto identity:*").await;
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

    // A PLC operation rewrites the DID document, so the migration scope must
    // not reach it: `account:repo?action=manage` is what `importRepo` needs,
    // and a client granted migration capability would otherwise be able to
    // rotate the signing key and take the account.
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
    assert_eq!(status, Status::Forbidden, "body was {body}");
    assert_eq!(body["error"], "InsufficientScope", "body was {body}");

    // Nor does the narrower identity grant: `identity:handle` permits a handle
    // change, not full control of the document.
    let (access_token, key) = granular_access_token(&client, "atproto identity:handle").await;
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

/// `AccountStatus`, `AccountRepo` and the `AccessFull` pairings of
/// `AccountEmail` and `IdentityFull` require `AuthScope::Access` -- a scope
/// only a legacy plain-JWT `createSession` token carries (see the module doc
/// above). This is a regression check on the only path currently able to
/// reach them: a legacy session still clears `AccountEmail` exactly as it
/// cleared the plain `AccessFull` guard before the seam.
#[tokio::test]
async fn account_email_declaration_still_accepts_a_legacy_session() {
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

/// Dispatches an authenticated GET, retrying once with the server's DPoP
/// nonce the same way a real client would.
async fn dispatch_scoped_get(
    client: &Client,
    path: &str,
    access_token: &str,
    key: &Jwk,
) -> (Status, Value) {
    let htu = format!("{}{}", public_url(client), path);
    let send = |nonce: Option<String>| {
        client
            .get(path)
            .header(Header::new("Authorization", format!("DPoP {access_token}")))
            .header(Header::new(
                "DPoP",
                dpop_proof(key, "GET", &htu, nonce.as_deref(), Some(access_token)),
            ))
            .dispatch()
    };
    let first = send(None).await;
    let response = if first.status() == Status::Unauthorized {
        match first.headers().get_one("DPoP-Nonce").map(str::to_string) {
            Some(nonce) => send(Some(nonce)).await,
            None => first,
        }
    } else {
        first
    };
    let status = response.status();
    let text = response.into_string().await.unwrap_or_default();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

/// `com.atproto.repo.createRecord` declares `Scoped<RepoWrite, ...>`, whose
/// check is deferred: the handler names the collection it is about to write
/// and only then gets the requester back. A grant covering one collection
/// must not reach another.
#[tokio::test]
async fn repo_write_declaration_confines_the_collection_it_is_given() {
    let (_dir, client) = get_oauth_client().await;
    common::create_account(&client).await;
    activate_test_account(&client).await;
    let scope = "atproto repo:app.bsky.feed.post";

    // The granted collection reaches the handler.
    let (access_token, key) = granular_access_token(&client, scope).await;
    let (status, body) = dispatch_scoped_post(
        &client,
        "/xrpc/com.atproto.repo.createRecord",
        &access_token,
        &key,
        ContentType::JSON,
        json!({
            "repo": "did:web:missing.invalid",
            "collection": "app.bsky.feed.post",
            "record": { "$type": "app.bsky.feed.post", "text": "hi", "createdAt": "2024-01-01T00:00:00Z" }
        })
        .to_string()
        .into_bytes(),
    )
    .await;
    assert_ne!(status, Status::Forbidden, "body was {body}");
    assert_ne!(body["error"], "InsufficientScope", "body was {body}");

    // A different collection is refused before the handler runs at all.
    let (access_token, key) = granular_access_token(&client, scope).await;
    let (status, body) = dispatch_scoped_post(
        &client,
        "/xrpc/com.atproto.repo.createRecord",
        &access_token,
        &key,
        ContentType::JSON,
        json!({
            "repo": "did:web:missing.invalid",
            "collection": "app.bsky.feed.like",
            "record": { "$type": "app.bsky.feed.like" }
        })
        .to_string()
        .into_bytes(),
    )
    .await;
    assert_eq!(status, Status::Forbidden, "body was {body}");
    assert_eq!(body["error"], "InsufficientScope", "body was {body}");
}

/// `com.atproto.server.getSession` declares `Scoped<NoScopeRequired>` -- the
/// always-allowed opt-out. A session whose only resource grant is unrelated
/// still reaches it.
#[tokio::test]
async fn no_scope_required_declaration_permits_a_narrowly_scoped_session() {
    let (_dir, client) = get_oauth_client().await;
    common::create_account(&client).await;
    activate_test_account(&client).await;

    let (access_token, key) = granular_access_token(&client, "atproto blob:image/*").await;
    let (status, body) = dispatch_scoped_get(
        &client,
        "/xrpc/com.atproto.server.getSession",
        &access_token,
        &key,
    )
    .await;
    assert_eq!(status, Status::Ok, "body was {body}");
    assert!(body["did"].as_str().is_some(), "body was {body}");
}

/// `com.atproto.server.listAppPasswords` declares `Scoped<OAuthForbidden, ...>`.
/// The declaration must not disturb the legacy sessions that are the only
/// callers it admits.
#[tokio::test]
async fn oauth_forbidden_declaration_still_accepts_a_legacy_session() {
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
        .get("/xrpc/com.atproto.server.listAppPasswords")
        .header(Header::new("Authorization", format!("Bearer {access_jwt}")))
        .dispatch()
        .await;
    let status = response.status();
    let text = response.into_string().await.unwrap_or_default();
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    assert_eq!(status, Status::Ok, "body was {body}");
    assert_ne!(body["error"], "InsufficientScope", "body was {body}");
}

/// A second-factor gate in front of the sign-in route sends the browser
/// back with a hint and, after a bad code, an error; the form then carries
/// the code, which this server accepts and ignores.
/// The gate also sends the browser back with `auth_error=true` when it
/// refused the credentials of an account it will not forward without a
/// code; the page shows the same error the PDS would, and no code field.
#[tokio::test]
async fn the_sign_in_page_shows_rejected_credentials_when_told_to() {
    let (_dir, client) = get_oauth_client().await;
    common::create_account(&client).await;
    activate_test_account(&client).await;
    let key = dpop_key();
    let (request_uri, _nonce) = run_par(&client, &key).await;
    let path = format!(
        "{}&auth_error=true",
        authorize_path(LOOPBACK_CLIENT_ID, &request_uri)
    );
    let response = client
        .get(path)
        .header(Header::new("Accept", "text/html"))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Invalid identifier or password"), "{html}");
    assert!(html.contains("class=\"error\""));
    assert!(!html.contains("name=\"email_otp\""));
}

#[tokio::test]
async fn the_sign_in_page_shows_a_second_factor_field_when_told_to() {
    let (_dir, client) = get_oauth_client().await;
    let key = dpop_key();
    let (request_uri, _nonce) = run_par(&client, &key).await;
    let response = client
        .get(format!(
            "{}&otp_hint=a%2A%2A%2A%40example.test&otp_error=true",
            authorize_path(LOOPBACK_CLIENT_ID, &request_uri)
        ))
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
    assert!(html.contains("name=\"email_otp\""), "{html}");
    assert!(html.contains("a***@example.test"));
    assert!(html.contains("The sign-in code was not accepted"));
    let csrf = extract_csrf(&html);
    let signed_in = client
        .post("/oauth/authorize/sign-in")
        .header(ContentType::Form)
        .cookie(("device-id", cookie))
        .body(form_encode(&[
            ("client_id", LOOPBACK_CLIENT_ID),
            ("request_uri", request_uri.as_str()),
            ("csrf", csrf.as_str()),
            ("identifier", "nobody.rsky.com"),
            ("password", "wrong"),
            ("email_otp", "12345"),
        ]))
        .dispatch()
        .await;
    assert_eq!(signed_in.status(), Status::Ok);
    let html = signed_in.into_string().await.unwrap();
    assert!(html.contains("class=\"error\""), "{html}");
    assert!(!html.contains("name=\"email_otp\""));
}

/// PAR with extra request parameters (a prompt, a login hint), after the
/// nonce challenge. Returns the request_uri and the fresh server nonce.
async fn run_par_with(
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

async fn get_authorize_html(client: &Client, path: String, session: &AuthorizeSession) -> String {
    let response = client
        .get(path)
        .cookie(("device-id", session.cookie.clone()))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    response.into_string().await.unwrap()
}

/// The picker offers "Another account", which shows the plain form; a hint
/// naming a signed-in account that still needs consent goes straight to
/// the consent screen; the branded shell carries the page headers.
#[tokio::test]
async fn oauth_sign_in_page_variants_after_a_session_exists() {
    let (_dir, client) = get_oauth_client().await;
    common::create_account(&client).await;
    let key = dpop_key();
    let (request_uri, nonce) = run_par(&client, &key).await;
    let mut session = open_authorize_page(&client, &request_uri).await;
    let code = sign_in_and_accept(&client, &request_uri, &mut session).await;
    exchange_code(&client, &key, &code, &nonce).await;

    let (request_uri, _) = run_par(&client, &key).await;
    let html = get_authorize_html(
        &client,
        authorize_path(LOOPBACK_CLIENT_ID, &request_uri),
        &session,
    )
    .await;
    assert!(html.contains("Sign in as..."), "{html}");
    assert!(html.contains("Another account"));
    assert!(html.contains("&amp;view=sign-in"));
    assert!(!html.contains("name=\"password\""));

    let html = get_authorize_html(
        &client,
        format!(
            "{}&view=sign-in",
            authorize_path(LOOPBACK_CLIENT_ID, &request_uri)
        ),
        &session,
    )
    .await;
    assert!(html.contains("name=\"password\""), "{html}");
    assert!(html.contains("Enter your username and password"));
    assert!(html.contains(">Back</a>"));
    assert!(!html.contains("Another account"));

    // the hint names the session and the client asks for consent again,
    // so the consent screen comes up without the picker
    let (request_uri, _) = run_par_with(
        &client,
        &key,
        LOOPBACK_CLIENT_ID,
        "atproto transition:generic",
        &[
            ("login_hint", "did:plc:khvyd3oiw46vif5gm7hijslk"),
            ("prompt", "consent"),
        ],
    )
    .await;
    let response = client
        .get(authorize_path(LOOPBACK_CLIENT_ID, &request_uri))
        .cookie(("device-id", session.cookie.clone()))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let csp = response
        .headers()
        .get_one("Content-Security-Policy")
        .expect("pages carry a CSP")
        .to_string();
    assert!(csp.contains("default-src 'none'"));
    assert_eq!(
        response.headers().get_one("Cache-Control"),
        Some("no-store")
    );
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Grant access to your"), "{html}");
    assert!(html.contains("Full access to your account data"));
    assert!(!html.contains("name=\"password\""));

    // a hint naming an account this device never signed in fixes the
    // identifier on the form
    let (request_uri, _) = run_par_with(
        &client,
        &key,
        LOOPBACK_CLIENT_ID,
        "atproto transition:generic",
        &[("login_hint", "someone.else.test")],
    )
    .await;
    let html = get_authorize_html(
        &client,
        authorize_path(LOOPBACK_CLIENT_ID, &request_uri),
        &session,
    )
    .await;
    assert!(html.contains("Enter your password"), "{html}");
    assert!(html.contains("value=\"someone.else.test\""));
    assert!(html.contains("readonly"));
    assert!(!html.contains("Another account"));
}

/// Picking a session whose authentication no longer counts asks for the
/// password again instead of showing consent.
#[tokio::test]
async fn oauth_select_stale_session_confirms_the_password() {
    let (_dir, client) = get_oauth_client().await;
    common::create_account(&client).await;
    let key = dpop_key();
    let (request_uri, nonce) = run_par(&client, &key).await;
    let mut session = open_authorize_page(&client, &request_uri).await;
    let code = sign_in_and_accept(&client, &request_uri, &mut session).await;
    exchange_code(&client, &key, &code, &nonce).await;

    let select = |request_uri: String, session: &AuthorizeSession| {
        client
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
    };

    // an authentication older than the maximum age is stale
    let device_id = session.cookie.split_once('.').unwrap().0.to_string();
    let shared = client
        .rocket()
        .state::<rsky_pds::oauth::SharedOAuthProvider>()
        .unwrap();
    shared
        .provider
        .store()
        .upsert_device_account(
            &device_id,
            "did:plc:khvyd3oiw46vif5gm7hijslk",
            now_secs() - rsky_oauth::store::AUTHENTICATION_MAX_AGE - 60,
        )
        .await
        .unwrap();
    let (request_uri, _) = run_par(&client, &key).await;
    let html = get_authorize_html(
        &client,
        authorize_path(LOOPBACK_CLIENT_ID, &request_uri),
        &session,
    )
    .await;
    assert!(html.contains("Login required"), "{html}");
    let response = select(request_uri, &session).await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Confirm your password to continue"), "{html}");
    assert!(html.contains("value=\"foo.rsky.com\""));
    assert!(html.contains("readonly"));
}

/// The consent page offers to decline the email grant; an unchecked box
/// narrows what the token carries.
#[tokio::test]
async fn oauth_consent_can_decline_the_email_grant() {
    let (_dir, client) = get_oauth_client().await;
    common::create_account(&client).await;
    let key = dpop_key();
    let scope = "atproto account:email repo:app.bsky.feed.post";
    let client_id = loopback_client_id(scope);
    let (request_uri, nonce) = run_par_scoped(&client, &key, &client_id, scope).await;
    let mut session = open_authorize_page_scoped(&client, &client_id, &request_uri).await;
    let response = client
        .post("/oauth/authorize/sign-in")
        .header(ContentType::Form)
        .cookie(("device-id", session.cookie.clone()))
        .body(form_encode(&[
            ("request_uri", &request_uri),
            ("client_id", &client_id),
            ("csrf", &session.csrf),
            ("identifier", "foo@example.com"),
            ("password", "password"),
        ]))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    session.cookie = response
        .cookies()
        .get("device-id")
        .unwrap()
        .value()
        .to_string();
    let html = response.into_string().await.unwrap();
    assert!(
        html.contains("name=\"email_optional\" value=\"1\""),
        "{html}"
    );
    assert!(html.contains("name=\"allow_email\""));
    assert!(html.contains("Read your account's email address"));
    session.csrf = extract_csrf(&html);

    let response = client
        .post("/oauth/authorize/accept")
        .header(ContentType::Form)
        .cookie(("device-id", session.cookie.clone()))
        .body(form_encode(&[
            ("request_uri", &request_uri),
            ("client_id", &client_id),
            ("csrf", &session.csrf),
            ("did", "did:plc:khvyd3oiw46vif5gm7hijslk"),
            ("scope", scope),
            ("email_optional", "1"),
        ]))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::SeeOther);
    let location = response.headers().get_one("Location").unwrap().to_string();
    let url = url::Url::parse(&location).unwrap();
    let code = url
        .query_pairs()
        .find(|(key, _)| key == "code")
        .map(|(_, value)| value.into_owned())
        .unwrap();
    let token = exchange_code_scoped(&client, &client_id, &key, &code, &nonce).await;
    assert_eq!(token["scope"], "atproto repo:app.bsky.feed.post");
}

/// Accepting without naming the account is refused on the error page.
#[tokio::test]
async fn oauth_accept_requires_a_did() {
    let (_dir, client) = get_oauth_client().await;
    common::create_account(&client).await;
    let key = dpop_key();
    let (request_uri, _) = run_par(&client, &key).await;
    let session = open_authorize_page(&client, &request_uri).await;
    let response = client
        .post("/oauth/authorize/accept")
        .header(ContentType::Form)
        .cookie(("device-id", session.cookie.clone()))
        .body(form_encode(&[
            ("request_uri", &request_uri),
            ("client_id", LOOPBACK_CLIENT_ID),
            ("csrf", &session.csrf),
        ]))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::BadRequest);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("did is required"), "{html}");
    assert!(html.contains("An error occurred"));
}

/// The stylesheet the pages link to is served under its content hash.
#[tokio::test]
async fn oauth_pages_link_a_served_stylesheet() {
    let (_dir, client) = get_oauth_client().await;
    common::create_account(&client).await;
    let key = dpop_key();
    let (request_uri, _) = run_par(&client, &key).await;
    let response = client
        .get(authorize_path(LOOPBACK_CLIENT_ID, &request_uri))
        .dispatch()
        .await;
    let html = response.into_string().await.unwrap();
    let marker = "<link rel=\"stylesheet\" href=\"";
    let start = html.find(marker).unwrap() + marker.len();
    let href = &html[start..start + html[start..].find('"').unwrap()];
    assert!(href.starts_with("/oauth/assets/ui-"), "{href}");
    let response = client.get(href).dispatch().await;
    assert_eq!(response.status(), Status::Ok);
    assert_eq!(
        response.headers().get_one("Cache-Control"),
        Some("public, max-age=31536000, immutable")
    );
    assert!(response
        .into_string()
        .await
        .unwrap()
        .contains("--branding-color-primary"));
    let response = client.get("/oauth/assets/ui-0000.css").dispatch().await;
    assert_eq!(response.status(), Status::NotFound);
}
