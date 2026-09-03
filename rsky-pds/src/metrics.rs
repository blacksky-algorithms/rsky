//! Prometheus metrics for rsky-pds.
//!
//! Modeled directly on `rsky-relay`'s `metrics.rs`: the `metrics` facade
//! (idempotent `describe_*!` registration + free-standing `record_*`
//! helpers) plus a `metrics-exporter-prometheus` recorder. Unlike the relay
//! -- which binds its own bare TCP listener -- rsky-pds is Rocket-based, so
//! the recorder is exposed through a normal Rocket route (see
//! [`metrics_route`]) instead of a second socket.
//!
//! Metric names intentionally echo the attribute names the TypeScript
//! reference PDS attaches to its XRPC spans/metrics (the lexicon NSID as
//! `method`, the HTTP status as `status`), translated into Prometheus'
//! label conventions so the two implementations stay comparable side by
//! side.

use std::sync::OnceLock;
use std::time::Instant;

use metrics::{
    counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram, Unit,
};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use rocket::fairing::{Fairing, Info, Kind};
use rocket::http::ContentType;
use rocket::{Data, Request, Response};

/// Count of XRPC requests, labelled by lexicon method (NSID) and HTTP status code.
pub const XRPC_REQUESTS: &str = "pds_xrpc_requests_total";
/// XRPC request latency in seconds, labelled by lexicon method and HTTP status code.
pub const XRPC_REQUEST_DURATION_SECONDS: &str = "pds_xrpc_request_duration_seconds";
/// Login attempts via `com.atproto.server.createSession`, labelled by outcome.
pub const AUTH_LOGIN: &str = "pds_auth_login_total";
/// Service-auth tokens minted via `com.atproto.server.getServiceAuth`.
pub const AUTH_SERVICE_TOKENS_ISSUED: &str = "pds_auth_service_tokens_issued_total";
/// Repo writes (record creates/updates/deletes) committed to an actor store.
pub const REPO_WRITES: &str = "pds_repo_writes_total";
/// Successful blob uploads via `com.atproto.repo.uploadBlob`.
pub const BLOB_UPLOADS: &str = "pds_blob_uploads_total";
/// Bytes accepted across all blob uploads.
pub const BLOB_UPLOAD_BYTES: &str = "pds_blob_upload_bytes_total";
/// Currently-connected `com.atproto.sync.subscribeRepos` (firehose) subscribers.
pub const FIREHOSE_SUBSCRIBERS: &str = "pds_firehose_subscribers";

/// Register all rsky-pds metrics with descriptions. Idempotent: `describe_*!`
/// macros just re-set the same description on repeated calls, so this is
/// safe to call from multiple `build_rocket()` invocations (e.g. in tests).
pub fn describe() {
    describe_counter!(
        XRPC_REQUESTS,
        Unit::Count,
        "XRPC requests handled, by lexicon method and HTTP status"
    );
    describe_histogram!(
        XRPC_REQUEST_DURATION_SECONDS,
        Unit::Seconds,
        "XRPC request latency, by lexicon method and HTTP status"
    );
    describe_counter!(
        AUTH_LOGIN,
        Unit::Count,
        "createSession login attempts, by outcome (success/failure)"
    );
    describe_counter!(
        AUTH_SERVICE_TOKENS_ISSUED,
        Unit::Count,
        "getServiceAuth service-auth tokens issued"
    );
    describe_counter!(
        REPO_WRITES,
        Unit::Count,
        "Repo record writes committed, by operation (create/update/delete)"
    );
    describe_counter!(BLOB_UPLOADS, Unit::Count, "Successful blob uploads");
    describe_counter!(
        BLOB_UPLOAD_BYTES,
        Unit::Bytes,
        "Bytes accepted across all blob uploads"
    );
    describe_gauge!(
        FIREHOSE_SUBSCRIBERS,
        Unit::Count,
        "Currently-connected subscribeRepos (firehose) subscribers"
    );
}

static PROMETHEUS_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Install the process-wide Prometheus recorder and return a handle to it.
///
/// Idempotent and safe to call from multiple `build_rocket()` invocations:
/// the actual recorder is only ever built once per process (guarded by a
/// `OnceLock`), so every caller -- including concurrent tests that each spin
/// up their own Rocket instance -- gets a handle to the *same* underlying
/// recorder rather than a disconnected one.
pub fn install_recorder() -> PrometheusHandle {
    PROMETHEUS_HANDLE
        .get_or_init(|| {
            let recorder = PrometheusBuilder::new().build_recorder();
            let handle = recorder.handle();
            // Tolerate "global recorder already set" (e.g. another crate in
            // the same process installed one first). This closure only ever
            // runs once per process (guarded by the OnceLock), so we always
            // win the race for our own handle/recorder pair.
            let _ = metrics::set_global_recorder(recorder);
            describe();
            handle
        })
        .clone()
}

struct RequestStart(Instant);

/// Rocket fairing that records XRPC request count + latency, labelled by
/// lexicon method (the NSID segment of the `/xrpc/<nsid>` path) and HTTP
/// status code.
pub struct XrpcMetrics;

#[rocket::async_trait]
impl Fairing for XrpcMetrics {
    fn info(&self) -> Info {
        Info {
            name: "XRPC request metrics",
            kind: Kind::Request | Kind::Response,
        }
    }

    async fn on_request(&self, request: &mut Request<'_>, _data: &mut Data<'_>) {
        request.local_cache(|| RequestStart(Instant::now()));
    }

    async fn on_response<'r>(&self, request: &'r Request<'_>, response: &mut Response<'r>) {
        let path = request.uri().path();
        let Some(method) = path.as_str().strip_prefix("/xrpc/") else {
            return;
        };
        let method = method.to_string();
        let status = response.status().code.to_string();
        let start = request.local_cache(|| RequestStart(Instant::now()));
        let elapsed = start.0.elapsed().as_secs_f64();
        counter!(XRPC_REQUESTS, "method" => method.clone(), "status" => status.clone())
            .increment(1);
        histogram!(XRPC_REQUEST_DURATION_SECONDS, "method" => method, "status" => status)
            .record(elapsed);
    }
}

/// `/metrics` route handler: renders the process' Prometheus recorder as
/// plain-text exposition format.
#[rocket::get("/metrics")]
pub async fn metrics_route(handle: &rocket::State<PrometheusHandle>) -> (ContentType, String) {
    (
        ContentType::new("text", "plain").with_params(("version", "0.0.4")),
        handle.render(),
    )
}

#[inline]
pub fn record_login(success: bool) {
    let outcome = if success { "success" } else { "failure" };
    counter!(AUTH_LOGIN, "outcome" => outcome).increment(1);
}

#[inline]
pub fn record_service_token_issued() {
    counter!(AUTH_SERVICE_TOKENS_ISSUED).increment(1);
}

#[inline]
pub fn record_repo_write(op: &'static str) {
    counter!(REPO_WRITES, "op" => op).increment(1);
}

#[inline]
pub fn record_blob_upload(bytes: u64) {
    counter!(BLOB_UPLOADS).increment(1);
    counter!(BLOB_UPLOAD_BYTES).increment(bytes);
}

#[inline]
pub fn record_firehose_subscriber_connected() {
    gauge!(FIREHOSE_SUBSCRIBERS).increment(1.0);
}

#[inline]
pub fn record_firehose_subscriber_disconnected() {
    gauge!(FIREHOSE_SUBSCRIBERS).decrement(1.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use metrics::with_local_recorder;
    use metrics_exporter_prometheus::PrometheusBuilder;

    #[test]
    fn describe_is_idempotent_under_local_recorder() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        with_local_recorder(&recorder, || {
            describe();
            describe();
        });
        // render() must not panic; output is plain Prometheus text or empty.
        let _out = handle.render();
    }

    #[test]
    fn record_login_increments_labelled_counter() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        with_local_recorder(&recorder, || {
            describe();
            record_login(true);
            record_login(true);
            record_login(false);
        });
        let out = handle.render();
        assert!(out.contains(AUTH_LOGIN), "missing metric: {out}");
        assert!(out.contains("outcome=\"success\""));
        assert!(out.contains("outcome=\"failure\""));
    }

    #[test]
    fn record_service_token_issued_increments_counter() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        with_local_recorder(&recorder, || {
            describe();
            record_service_token_issued();
            record_service_token_issued();
        });
        let out = handle.render();
        assert!(out.contains(AUTH_SERVICE_TOKENS_ISSUED));
        assert!(out.contains(&format!("{AUTH_SERVICE_TOKENS_ISSUED} 2")));
    }

    #[test]
    fn record_repo_write_uses_op_label() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        with_local_recorder(&recorder, || {
            describe();
            record_repo_write("create");
            record_repo_write("update");
            record_repo_write("delete");
        });
        let out = handle.render();
        assert!(out.contains("op=\"create\""));
        assert!(out.contains("op=\"update\""));
        assert!(out.contains("op=\"delete\""));
    }

    #[test]
    fn record_blob_upload_increments_count_and_bytes() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        with_local_recorder(&recorder, || {
            describe();
            record_blob_upload(1_024);
            record_blob_upload(2_048);
        });
        let out = handle.render();
        assert!(out.contains(&format!("{BLOB_UPLOADS} 2")));
        assert!(out.contains(&format!("{BLOB_UPLOAD_BYTES} 3072")));
    }

    #[test]
    fn firehose_subscriber_gauge_tracks_connect_disconnect() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        with_local_recorder(&recorder, || {
            describe();
            record_firehose_subscriber_connected();
            record_firehose_subscriber_connected();
            record_firehose_subscriber_disconnected();
        });
        let out = handle.render();
        assert!(out.contains(FIREHOSE_SUBSCRIBERS));
        assert!(out.contains(&format!("{FIREHOSE_SUBSCRIBERS} 1")));
    }

    #[test]
    fn install_recorder_is_idempotent_and_returns_working_handle() {
        // Calling twice in the same process must not panic, and both handles
        // must reflect the same underlying (process-wide) recorder.
        let h1 = install_recorder();
        let h2 = install_recorder();
        record_service_token_issued();
        let out1 = h1.render();
        let out2 = h2.render();
        assert_eq!(out1, out2);
        assert!(out1.contains(AUTH_SERVICE_TOKENS_ISSUED));
    }
}
