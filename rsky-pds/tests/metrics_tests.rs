//! Integration tests for the `/metrics` Prometheus endpoint and the XRPC
//! request fairing that feeds it, exercised against a real Rocket instance
//! (mirroring how `rsky-relay`'s metrics tests exercise its recorder).

mod common;

use rocket::http::{ContentType, Status};
use rocket::serde::json::json;

/// `/metrics` must exist, respond 200, and render valid-looking Prometheus
/// exposition text (not JSON, not empty of our metric names).
#[rocket::async_test]
async fn metrics_endpoint_returns_prometheus_text() {
    let (_dir, client) = common::get_client().await;

    let response = client.get("/metrics").dispatch().await;
    assert_eq!(response.status(), Status::Ok);

    let content_type = response.content_type().expect("content-type header set");
    assert_eq!(content_type.top(), "text");
    assert_eq!(content_type.sub(), "plain");

    // `metrics-exporter-prometheus` only emits a metric family once it has
    // at least one recorded sample, so an otherwise-idle server can validly
    // render an empty (but well-formed) body here; the "metrics actually
    // show up" assertions live in `xrpc_requests_are_counted_by_method_and_status`.
    let body = response.into_string().await.expect("response body");
    assert!(
        body.is_ascii(),
        "Prometheus exposition text must be ASCII: {body}"
    );
}

/// Real requests flowing through the Rocket app must show up in `/metrics`,
/// labelled by lexicon method and status code -- this is the fairing wired
/// into `build_rocket()`, not just the unit-level `record_*` helpers.
#[rocket::async_test]
async fn xrpc_requests_are_counted_by_method_and_status() {
    let (_dir, client) = common::get_client().await;

    // A cheap, auth-free XRPC route that never touches the database.
    let health_response = client.get("/xrpc/_health/live").dispatch().await;
    assert_eq!(health_response.status(), Status::Ok);

    // A login attempt against a nonexistent account: exercises the
    // `pds_session_created_total{source="password",outcome="failure"}` hook
    // in `create_session`.
    let login_response = client
        .post("/xrpc/com.atproto.server.createSession")
        .header(ContentType::JSON)
        .body(json!({"identifier": "nobody.test", "password": "wrong"}).to_string())
        .dispatch()
        .await;
    assert_eq!(login_response.status(), Status::BadRequest);

    let metrics_response = client.get("/metrics").dispatch().await;
    let body = metrics_response.into_string().await.expect("response body");

    assert!(
        body.contains(r#"method="_health/live""#),
        "expected _health/live method label in: {body}"
    );
    assert!(
        body.contains(r#"status="200""#),
        "expected status=\"200\" label in: {body}"
    );
    assert!(body.contains("pds_xrpc_request_duration_seconds"), "{body}");
    assert!(body.contains("pds_xrpc_requests_total"), "{body}");
    assert!(
        body.contains(r#"method="com.atproto.server.createSession""#),
        "{body}"
    );
    assert!(body.contains(r#"source="password""#), "{body}");
    assert!(body.contains(r#"outcome="failure""#), "{body}");
    assert!(
        body.contains(r#"reason="invalid_credentials""#),
        "expected the createSession failure to carry a reason label: {body}"
    );
    assert!(body.contains("pds_session_created_total"), "{body}");
}

/// `/oauth/*` isn't behind an `/xrpc/` prefix, but it's a fixed, low-
/// cardinality route set (see `metrics::route_label`) and should be counted
/// the same way XRPC requests are.
#[rocket::async_test]
async fn oauth_requests_are_counted_too() {
    let (_dir, client) = common::get_client().await;

    let jwks_response = client.get("/oauth/jwks").dispatch().await;
    assert_eq!(jwks_response.status(), Status::Ok);

    let metrics_response = client.get("/metrics").dispatch().await;
    let body = metrics_response.into_string().await.expect("response body");

    assert!(
        body.contains(r#"method="/oauth/jwks""#),
        "expected /oauth/jwks to be recorded under the xrpc/oauth request counter: {body}"
    );
}

/// `common::create_account` drives `createAccount` with an admin token and
/// an invite code (mirroring how a real invite-gated PDS provisions
/// accounts), landing deactivated pending activation -- so the counter
/// should show `source="admin"`, `invited="true"`, `deactivated="true"`.
#[rocket::async_test]
async fn account_created_is_counted_by_source() {
    let (_dir, client) = common::get_client().await;

    common::create_account(&client).await;

    let metrics_response = client.get("/metrics").dispatch().await;
    let body = metrics_response.into_string().await.expect("response body");

    assert!(
        body.contains(
            r#"pds_accounts_created_total{source="admin",invited="true",deactivated="true"} 1"#
        ),
        "{body}"
    );
}
