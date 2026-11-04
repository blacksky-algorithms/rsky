//! Prometheus metrics, served on their own port.

use prometheus::{Encoder, HistogramVec, IntCounter, IntCounterVec, Registry, TextEncoder};
use std::sync::LazyLock;

pub struct Metrics {
    registry: Registry,
    pub requests: IntCounterVec,
    pub latency: HistogramVec,
    pub writes_rejected: IntCounterVec,
    pub journal_failures: IntCounter,
    pub misroutes: IntCounter,
    pub failovers: IntCounterVec,
}

pub static METRICS: LazyLock<Metrics> = LazyLock::new(Metrics::new);

impl Metrics {
    fn new() -> Self {
        let registry = Registry::new();
        let requests = IntCounterVec::new(
            prometheus::Opts::new(
                "router_requests_total",
                "Requests by backend, class, and status",
            ),
            &["backend", "class", "status"],
        )
        .expect("valid metric");
        let latency = HistogramVec::new(
            prometheus::HistogramOpts::new(
                "router_request_duration_seconds",
                "Time from request receipt to response headers",
            )
            .buckets(vec![
                0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0,
            ]),
            &["backend", "class"],
        )
        .expect("valid metric");
        let writes_rejected = IntCounterVec::new(
            prometheus::Opts::new(
                "router_writes_rejected_total",
                "Mutations refused, by reason",
            ),
            &["reason"],
        )
        .expect("valid metric");
        let journal_failures = IntCounter::new(
            "router_journal_failures_total",
            "Mutations refused because the journal could not be written",
        )
        .expect("valid metric");
        let misroutes = IntCounter::new(
            "router_misroute_total",
            "Requests whose backend could not be decided",
        )
        .expect("valid metric");
        let failovers = IntCounterVec::new(
            prometheus::Opts::new(
                "router_failovers_total",
                "Reads answered by the fallback pool after a connection failure",
            ),
            &["class"],
        )
        .expect("valid metric");
        for collector in [
            Box::new(requests.clone()) as Box<dyn prometheus::core::Collector>,
            Box::new(latency.clone()),
            Box::new(writes_rejected.clone()),
            Box::new(journal_failures.clone()),
            Box::new(misroutes.clone()),
            Box::new(failovers.clone()),
        ] {
            registry.register(collector).expect("unique metric");
        }
        Self {
            registry,
            requests,
            latency,
            writes_rejected,
            journal_failures,
            misroutes,
            failovers,
        }
    }

    pub fn render(&self) -> String {
        let mut out = Vec::new();
        TextEncoder::new()
            .encode(&self.registry.gather(), &mut out)
            .expect("metrics encode");
        String::from_utf8_lossy(&out).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_every_family() {
        let metrics = Metrics::new();
        metrics
            .requests
            .with_label_values(&["ts", "sync", "200"])
            .inc();
        metrics
            .latency
            .with_label_values(&["ts", "sync"])
            .observe(0.02);
        metrics.writes_rejected.with_label_values(&["fenced"]).inc();
        metrics.journal_failures.inc();
        metrics.misroutes.inc();
        metrics.failovers.with_label_values(&["bsky"]).inc();
        let text = metrics.render();
        for needle in [
            "router_requests_total{backend=\"ts\",class=\"sync\",status=\"200\"} 1",
            "router_request_duration_seconds_count{backend=\"ts\",class=\"sync\"} 1",
            "router_writes_rejected_total{reason=\"fenced\"} 1",
            "router_journal_failures_total 1",
            "router_misroute_total 1",
            "router_failovers_total{class=\"bsky\"} 1",
        ] {
            assert!(text.contains(needle), "{needle}\n{text}");
        }
        assert!(METRICS.render().contains("router_journal_failures_total"));
    }
}
