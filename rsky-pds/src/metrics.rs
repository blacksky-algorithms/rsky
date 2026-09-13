//! Process-wide Prometheus metrics, served on `GET /metrics`.
//!
//! Counters and histograms are updated where the work happens; the gauges
//! that describe outstanding work are computed when scraped, from the same
//! journals the drain status reads.

use crate::actor_store::ActorStore;
use crate::lifecycle::LifecycleStore;
use crate::repair::RepairStore;
use crate::sequencer::Sequencer;
use anyhow::Result;
use prometheus::{
    Encoder, Histogram, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGauge,
    IntGaugeVec, Opts, Registry, TextEncoder,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;

pub struct Metrics {
    registry: Registry,
    pub http_requests: IntCounterVec,
    pub http_request_duration: HistogramVec,
    pub firehose_subscribers: IntGauge,
    pub sqlite_busy_retries: IntCounter,
    pub write_attempts: IntCounter,
    pub control_journal_writes: IntCounterVec,
    pub auth_failures: IntCounterVec,
    pub rate_limit_store_errors: IntCounter,
    pub blob_collector_outcomes: IntCounterVec,
    pub sequencer_last_seq: IntGauge,
    pub inflight_mutations: IntGaugeVec,
    pub publish_intents_pending: IntGaugeVec,
    pub blob_work_nonterminal: IntGaugeVec,
    pub pending_work_accounts: IntGauge,
    pub lifecycle_pending: IntGauge,
    pub repair_pending: IntGauge,
    pub export_duration: Histogram,
    pub shutting_down: IntGauge,
    pub xrpc_requests: IntCounterVec,
    pub xrpc_request_duration: HistogramVec,
    pub session_created: IntCounterVec,
    pub auth_service_tokens_issued: IntCounter,
    pub auth_service_tokens_denied: IntCounterVec,
    pub repo_writes: IntCounterVec,
    pub blob_uploads: IntCounter,
    pub blob_upload_bytes: IntCounter,
    pub accounts_created: IntCounterVec,
    pub oauth_authorization_grants: IntCounterVec,
    pub oauth_sessions_revoked: IntCounter,
    draining: AtomicBool,
}

pub static METRICS: LazyLock<Metrics> = LazyLock::new(Metrics::new);

fn counter_vec(registry: &Registry, name: &str, help: &str, labels: &[&str]) -> IntCounterVec {
    let vec = IntCounterVec::new(Opts::new(name, help), labels).expect("valid metric");
    registry
        .register(Box::new(vec.clone()))
        .expect("unique metric");
    vec
}

fn gauge_vec(registry: &Registry, name: &str, help: &str, labels: &[&str]) -> IntGaugeVec {
    let vec = IntGaugeVec::new(Opts::new(name, help), labels).expect("valid metric");
    registry
        .register(Box::new(vec.clone()))
        .expect("unique metric");
    vec
}

fn gauge(registry: &Registry, name: &str, help: &str) -> IntGauge {
    let gauge = IntGauge::new(name, help).expect("valid metric");
    registry
        .register(Box::new(gauge.clone()))
        .expect("unique metric");
    gauge
}

fn counter(registry: &Registry, name: &str, help: &str) -> IntCounter {
    let counter = IntCounter::new(name, help).expect("valid metric");
    registry
        .register(Box::new(counter.clone()))
        .expect("unique metric");
    counter
}

impl Metrics {
    fn new() -> Self {
        let registry = Registry::new();
        let http_request_duration = HistogramVec::new(
            HistogramOpts::new(
                "pds_http_request_duration_seconds",
                "Time from request receipt to response completion",
            )
            .buckets(vec![
                0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0,
            ]),
            &["route"],
        )
        .expect("valid metric");
        registry
            .register(Box::new(http_request_duration.clone()))
            .expect("unique metric");
        let xrpc_request_duration = HistogramVec::new(
            HistogramOpts::new(
                "pds_xrpc_request_duration_seconds",
                "XRPC and OAuth request latency, by lexicon method and status",
            )
            .buckets(vec![
                0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0,
            ]),
            &["method", "status"],
        )
        .expect("valid metric");
        registry
            .register(Box::new(xrpc_request_duration.clone()))
            .expect("unique metric");
        let export_duration = Histogram::with_opts(
            HistogramOpts::new(
                "pds_repo_export_duration_seconds",
                "Time to stream a repository export",
            )
            .buckets(vec![0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0, 300.0, 600.0]),
        )
        .expect("valid metric");
        registry
            .register(Box::new(export_duration.clone()))
            .expect("unique metric");
        Metrics {
            http_requests: counter_vec(
                &registry,
                "pds_http_requests_total",
                "Requests by route, method, and status",
                &["route", "method", "status"],
            ),
            http_request_duration,
            firehose_subscribers: gauge(
                &registry,
                "pds_firehose_subscribers",
                "Open subscribeRepos connections",
            ),
            sqlite_busy_retries: counter(
                &registry,
                "pds_sqlite_busy_retries_total",
                "Statements retried because sqlite reported busy",
            ),
            write_attempts: counter(
                &registry,
                "pds_write_attempts_total",
                "Actor store write transactions opened",
            ),
            control_journal_writes: counter_vec(
                &registry,
                "pds_control_journal_writes_total",
                "Writes to the rsky control journals by table",
                &["table"],
            ),
            auth_failures: counter_vec(
                &registry,
                "pds_auth_failures_total",
                "Rejected credentials by error",
                &["kind"],
            ),
            rate_limit_store_errors: counter(
                &registry,
                "pds_rate_limit_store_errors_total",
                "Rate limit consumptions allowed because the shared store did not answer",
            ),
            blob_collector_outcomes: counter_vec(
                &registry,
                "pds_blob_collector_outcomes_total",
                "Blob collector decisions by outcome",
                &["outcome"],
            ),
            sequencer_last_seq: gauge(
                &registry,
                "pds_sequencer_last_seq",
                "Highest sequence number emitted to subscribers",
            ),
            inflight_mutations: gauge_vec(
                &registry,
                "pds_inflight_mutations",
                "Write transactions in flight, by actor",
                &["did"],
            ),
            publish_intents_pending: gauge_vec(
                &registry,
                "pds_publish_intents_pending",
                "Commits not yet delivered to the sequencer, by actor",
                &["did"],
            ),
            blob_work_nonterminal: gauge_vec(
                &registry,
                "pds_blob_work_nonterminal",
                "Blob work still to run, by actor",
                &["did"],
            ),
            pending_work_accounts: gauge(
                &registry,
                "pds_pending_work_accounts",
                "Actors marked with undelivered work",
            ),
            lifecycle_pending: gauge(
                &registry,
                "pds_lifecycle_pending",
                "Account deletions not yet complete",
            ),
            repair_pending: gauge(&registry, "pds_repair_pending", "Repairs not yet terminal"),
            export_duration,
            shutting_down: gauge(
                &registry,
                "pds_shutting_down",
                "1 while the process drains before exit",
            ),
            xrpc_requests: counter_vec(
                &registry,
                "pds_xrpc_requests_total",
                "XRPC and OAuth requests, by lexicon method and status",
                &["method", "status"],
            ),
            xrpc_request_duration,
            session_created: counter_vec(
                &registry,
                "pds_session_created_total",
                "Sessions created, by source, outcome, and failure reason",
                &["source", "outcome", "reason"],
            ),
            auth_service_tokens_issued: counter(
                &registry,
                "pds_auth_service_tokens_issued_total",
                "Service-auth tokens issued by getServiceAuth",
            ),
            auth_service_tokens_denied: counter_vec(
                &registry,
                "pds_auth_service_tokens_denied_total",
                "Service-auth token requests denied, by reason",
                &["reason"],
            ),
            repo_writes: counter_vec(
                &registry,
                "pds_repo_writes_total",
                "Record writes committed, by operation",
                &["op"],
            ),
            blob_uploads: counter(
                &registry,
                "pds_blob_uploads_total",
                "Successful blob uploads",
            ),
            blob_upload_bytes: counter(
                &registry,
                "pds_blob_upload_bytes_total",
                "Bytes accepted across blob uploads",
            ),
            accounts_created: counter_vec(
                &registry,
                "pds_accounts_created_total",
                "Accounts created, by source, invitation, and deactivation",
                &["source", "invited", "deactivated"],
            ),
            oauth_authorization_grants: counter_vec(
                &registry,
                "pds_oauth_authorization_grants_total",
                "OAuth authorization grants, by client kind",
                &["client_first_party"],
            ),
            oauth_sessions_revoked: counter(
                &registry,
                "pds_oauth_sessions_revoked_total",
                "OAuth sessions revoked in bulk by a takedown or deletion",
            ),
            draining: AtomicBool::new(false),
            registry,
        }
    }

    pub fn control_journal_write(&self, table: &str) {
        self.control_journal_writes
            .with_label_values(&[table])
            .inc();
    }

    pub fn auth_failure(&self, kind: &str) {
        self.auth_failures.with_label_values(&[kind]).inc();
    }

    pub fn record_request(&self, route: &str, method: &str, status: u16, seconds: f64) {
        self.http_requests
            .with_label_values(&[route, method, &status.to_string()])
            .inc();
        self.http_request_duration
            .with_label_values(&[route])
            .observe(seconds);
    }

    /// Counts a routed request under the lexicon method it named; OAuth
    /// routes are counted under their literal path.
    pub fn record_xrpc(&self, path: &str, status: u16, seconds: f64) {
        let Some(method) = route_label(path) else {
            return;
        };
        let status = status.to_string();
        self.xrpc_requests
            .with_label_values(&[method, &status])
            .inc();
        self.xrpc_request_duration
            .with_label_values(&[method, &status])
            .observe(seconds);
    }

    /// Keeps the per-actor gauge to actors with a write in flight.
    pub fn set_inflight(&self, did: &str, count: usize) {
        if count == 0 {
            let _ = self.inflight_mutations.remove_label_values(&[did]);
        } else {
            self.inflight_mutations
                .with_label_values(&[did])
                .set(count as i64);
        }
    }

    pub fn begin_shutdown(&self) {
        self.draining.store(true, Ordering::SeqCst);
        self.shutting_down.set(1);
    }

    pub fn is_shutting_down(&self) -> bool {
        self.draining.load(Ordering::SeqCst)
    }

    /// Recomputes the outstanding-work gauges from the journals.
    pub async fn refresh(
        &self,
        actor_store: &ActorStore,
        lifecycle: &LifecycleStore,
        repairs: &RepairStore,
        sequencer: &Sequencer,
    ) -> Result<()> {
        self.sequencer_last_seq.set(sequencer.last_seen());
        self.lifecycle_pending
            .set(lifecycle.open_tombstones().await?.len() as i64);
        self.repair_pending.set(repairs.open_count().await?);
        let pending = lifecycle.pending_work().await?;
        self.pending_work_accounts.set(pending.len() as i64);
        self.publish_intents_pending.reset();
        self.blob_work_nonterminal.reset();
        for did in pending {
            let status = crate::drain::drain_status(actor_store, repairs, &did).await?;
            self.publish_intents_pending
                .with_label_values(&[&did])
                .set(status.publish_intent_pending as i64);
            self.blob_work_nonterminal
                .with_label_values(&[&did])
                .set(status.blob_work_nonterminal as i64);
        }
        Ok(())
    }

    /// The registry in the Prometheus text exposition format.
    pub fn render(&self) -> Result<String> {
        let mut out = Vec::new();
        TextEncoder::new().encode(&self.registry.gather(), &mut out)?;
        Ok(String::from_utf8(out)?)
    }
}

/// The lexicon method of an XRPC path, or the literal path of an OAuth
/// route; anything else is not counted by method.
fn route_label(path: &str) -> Option<&str> {
    if let Some(nsid) = path.strip_prefix("/xrpc/") {
        return Some(nsid);
    }
    if path.starts_with("/oauth/") || path.starts_with("/.well-known/oauth-") {
        return Some(path);
    }
    None
}

pub fn record_login_success(source: &str) {
    METRICS
        .session_created
        .with_label_values(&[source, "success", "n/a"])
        .inc();
}

pub fn record_login_failure(source: &str, reason: &str) {
    METRICS
        .session_created
        .with_label_values(&[source, "failure", reason])
        .inc();
}

pub fn record_service_token_issued() {
    METRICS.auth_service_tokens_issued.inc();
}

pub fn record_service_token_denied(reason: &str) {
    METRICS
        .auth_service_tokens_denied
        .with_label_values(&[reason])
        .inc();
}

pub fn record_repo_write(op: &str) {
    METRICS.repo_writes.with_label_values(&[op]).inc();
}

pub fn record_blob_upload(bytes: u64) {
    METRICS.blob_uploads.inc();
    METRICS.blob_upload_bytes.inc_by(bytes);
}

pub fn record_firehose_subscriber_connected() {
    METRICS.firehose_subscribers.inc();
}

pub fn record_firehose_subscriber_disconnected() {
    METRICS.firehose_subscribers.dec();
}

pub fn record_account_created(source: &str, invited: bool, deactivated: bool) {
    METRICS
        .accounts_created
        .with_label_values(&[source, &invited.to_string(), &deactivated.to_string()])
        .inc();
}

pub fn record_oauth_authorization_granted(client_first_party: bool) {
    METRICS
        .oauth_authorization_grants
        .with_label_values(&[&client_first_party.to_string()])
        .inc();
}

pub fn record_oauth_sessions_revoked(count: u64) {
    if count > 0 {
        METRICS.oauth_sessions_revoked.inc_by(count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_every_family_and_bounds_the_inflight_labels() {
        let metrics = Metrics::new();
        metrics.record_request("/xrpc/_health", "GET", 200, 0.01);
        metrics.control_journal_write("frontier_watermark");
        metrics.auth_failure("ExpiredToken");
        metrics.set_inflight("did:plc:a", 2);
        metrics.set_inflight("did:plc:b", 1);
        metrics.set_inflight("did:plc:b", 0);
        assert!(!metrics.is_shutting_down());
        metrics.begin_shutdown();
        assert!(metrics.is_shutting_down());
        let text = metrics.render().unwrap();
        for needle in [
            "pds_http_requests_total{method=\"GET\",route=\"/xrpc/_health\",status=\"200\"} 1",
            "pds_http_request_duration_seconds_count{route=\"/xrpc/_health\"} 1",
            "pds_control_journal_writes_total{table=\"frontier_watermark\"} 1",
            "pds_auth_failures_total{kind=\"ExpiredToken\"} 1",
            "pds_inflight_mutations{did=\"did:plc:a\"} 2",
            "pds_shutting_down 1",
            "pds_repo_export_duration_seconds_count 0",
        ] {
            assert!(text.contains(needle), "{needle} missing from:\n{text}");
        }
        assert!(!text.contains("did:plc:b"), "{text}");
    }

    #[test]
    fn the_reference_counters_render_with_their_labels() {
        let uploads_before = METRICS.blob_uploads.get();
        let revoked_before = METRICS.oauth_sessions_revoked.get();
        record_login_success("password");
        record_login_failure("oauth", "invalid_credentials");
        record_service_token_issued();
        record_service_token_denied("bad_expiration");
        record_repo_write("create");
        record_blob_upload(1_024);
        record_firehose_subscriber_connected();
        record_firehose_subscriber_disconnected();
        record_account_created("self_service", true, false);
        record_oauth_authorization_granted(true);
        record_oauth_sessions_revoked(0);
        record_oauth_sessions_revoked(2);
        METRICS.record_xrpc("/xrpc/com.atproto.server.getSession", 200, 0.01);
        METRICS.record_xrpc("/oauth/token", 400, 0.01);
        METRICS.record_xrpc("/robots.txt", 200, 0.01);
        assert_eq!(METRICS.blob_uploads.get(), uploads_before + 1);
        assert_eq!(METRICS.oauth_sessions_revoked.get(), revoked_before + 2);
        assert_eq!(
            route_label("/.well-known/oauth-authorization-server"),
            Some("/.well-known/oauth-authorization-server")
        );
        let out = METRICS.render().unwrap();
        for needle in [
            "pds_session_created_total{outcome=\"success\",reason=\"n/a\",source=\"password\"}",
            "pds_session_created_total{outcome=\"failure\",reason=\"invalid_credentials\",source=\"oauth\"}",
            "pds_auth_service_tokens_issued_total ",
            "pds_auth_service_tokens_denied_total{reason=\"bad_expiration\"}",
            "pds_repo_writes_total{op=\"create\"}",
            "pds_blob_upload_bytes_total ",
            "pds_accounts_created_total{deactivated=\"false\",invited=\"true\",source=\"self_service\"}",
            "pds_oauth_authorization_grants_total{client_first_party=\"true\"}",
            "pds_oauth_sessions_revoked_total ",
            "pds_xrpc_requests_total{method=\"com.atproto.server.getSession\",status=\"200\"}",
            "pds_xrpc_requests_total{method=\"/oauth/token\",status=\"400\"}",
            "pds_xrpc_request_duration_seconds_bucket{method=\"/oauth/token\",status=\"400\"",
        ] {
            assert!(out.contains(needle), "{needle}\n{out}");
        }
        assert!(!out.contains("method=\"/robots.txt\""));
    }
}
