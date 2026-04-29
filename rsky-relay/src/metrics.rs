use std::net::SocketAddr;

use metrics::{
    Unit, counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram,
};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use thiserror::Error;

pub const FRAMES_EMITTED: &str = "relay_frames_emitted_total";
pub const FRAMES_DROPPED: &str = "relay_frames_dropped_total";
pub const SUBSCRIBER_COUNT: &str = "relay_subscriber_count";
pub const SUBSCRIBER_LAG_SECONDS: &str = "relay_subscriber_lag_seconds";
pub const UPSTREAM_EVENTS: &str = "relay_upstream_events_total";
pub const UPSTREAM_DISCONNECTS: &str = "relay_upstream_disconnects_total";
pub const VALIDATOR_REJECTED: &str = "relay_validator_rejected_total";
pub const VALIDATOR_PUBLISHED: &str = "relay_validator_published_total";
pub const VALIDATOR_DEFERRED: &str = "relay_validator_deferred_total";
pub const VALIDATOR_DROPPED: &str = "relay_validator_dropped_total";
pub const VALIDATOR_PASSED_WITH_WARNING: &str = "relay_validator_passed_with_warning_total";
pub const FIREHOSE_HEAD: &str = "relay_firehose_head_seq";
pub const QUEUE_DEPTH_BYTES: &str = "relay_queue_depth_bytes";
pub const DISCOVERY_ROUND: &str = "relay_discovery_round_total";

#[derive(Debug, Clone, Copy)]
pub enum DropReason {
    CursorMismatch,
    WouldBlock,
    BufferFull,
    ConsumerTooSlow,
}

impl DropReason {
    #[inline]
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CursorMismatch => "cursor_mismatch",
            Self::WouldBlock => "would_block",
            Self::BufferFull => "buffer_full",
            Self::ConsumerTooSlow => "consumer_too_slow",
        }
    }
}

#[derive(Debug, Error)]
pub enum MetricsError {
    #[error("metrics builder error: {0}")]
    Builder(#[from] metrics_exporter_prometheus::BuildError),
}

/// Register all relay metrics with descriptions. Idempotent.
pub fn describe() {
    describe_counter!(FRAMES_EMITTED, Unit::Count, "Frames sent to subscribers");
    describe_counter!(FRAMES_DROPPED, Unit::Count, "Frames dropped before delivery");
    describe_gauge!(SUBSCRIBER_COUNT, Unit::Count, "Active subscribers");
    describe_histogram!(SUBSCRIBER_LAG_SECONDS, Unit::Seconds, "Subscriber lag behind live tail");
    describe_counter!(UPSTREAM_EVENTS, Unit::Count, "Events received from upstream PDS");
    describe_counter!(UPSTREAM_DISCONNECTS, Unit::Count, "Upstream PDS disconnects");
    describe_counter!(VALIDATOR_REJECTED, Unit::Count, "Events rejected by validator");
    describe_counter!(
        VALIDATOR_PUBLISHED,
        Unit::Count,
        "Events published to firehose by validator"
    );
    describe_counter!(VALIDATOR_DEFERRED, Unit::Count, "Events re-queued for later validation");
    describe_counter!(
        VALIDATOR_DROPPED,
        Unit::Count,
        "Events dropped without publish (strict mode)"
    );
    describe_counter!(
        VALIDATOR_PASSED_WITH_WARNING,
        Unit::Count,
        "Events published despite a soft validation failure (lenient mode)"
    );
    describe_gauge!(FIREHOSE_HEAD, Unit::Count, "Highest sequence number written to firehose");
    describe_gauge!(QUEUE_DEPTH_BYTES, Unit::Bytes, "Approximate validator queue partition size");
    describe_counter!(DISCOVERY_ROUND, Unit::Count, "listHosts discovery round outcome");
}

/// Build a recorder + handle without binding a socket. Tolerates "global already set".
pub fn install_recorder() -> Result<PrometheusHandle, MetricsError> {
    let recorder = PrometheusBuilder::new().build_recorder();
    let handle = recorder.handle();
    drop(metrics::set_global_recorder(recorder));
    describe();
    Ok(handle)
}

/// Bind an HTTP /metrics listener and install the recorder globally. Returns the handle.
/// MUST be called from within a tokio runtime — the exporter task is spawned there.
pub fn install_listener(addr: SocketAddr) -> Result<PrometheusHandle, MetricsError> {
    let (recorder, exporter) = PrometheusBuilder::new().with_http_listener(addr).build()?;
    let handle = recorder.handle();
    drop(metrics::set_global_recorder(recorder));
    describe();
    tokio::spawn(async move {
        if let Err(err) = exporter.await {
            tracing::error!(?err, "metrics exporter task ended");
        }
    });
    Ok(handle)
}

#[inline]
pub fn record_frame_emitted(frame_type: &'static str, op: i8) {
    counter!(FRAMES_EMITTED, "type" => frame_type, "op" => op.to_string()).increment(1);
}

#[inline]
pub fn record_frame_dropped(reason: DropReason) {
    counter!(FRAMES_DROPPED, "reason" => reason.as_str()).increment(1);
}

#[inline]
pub fn record_subscriber_count(n: i64) {
    #[expect(clippy::cast_precision_loss)]
    gauge!(SUBSCRIBER_COUNT).set(n as f64);
}

#[inline]
pub fn record_subscriber_lag_seconds(addr: &str, seconds: f64) {
    histogram!(SUBSCRIBER_LAG_SECONDS, "addr" => addr.to_owned()).record(seconds);
}

#[inline]
pub fn record_upstream_event(host: &str, kind: &'static str) {
    counter!(UPSTREAM_EVENTS, "host" => host.to_owned(), "type" => kind).increment(1);
}

#[inline]
pub fn record_upstream_disconnect(host: &str, reason: &'static str) {
    counter!(UPSTREAM_DISCONNECTS, "host" => host.to_owned(), "reason" => reason).increment(1);
}

#[inline]
pub fn record_validator_rejected(reason: &'static str) {
    counter!(VALIDATOR_REJECTED, "reason" => reason).increment(1);
}

#[inline]
pub fn record_validator_published(reason: &'static str) {
    counter!(VALIDATOR_PUBLISHED, "reason" => reason).increment(1);
}

#[inline]
pub fn record_validator_deferred(reason: &'static str) {
    counter!(VALIDATOR_DEFERRED, "reason" => reason).increment(1);
}

#[inline]
pub fn record_validator_dropped(reason: &'static str) {
    counter!(VALIDATOR_DROPPED, "reason" => reason).increment(1);
}

#[inline]
pub fn record_validator_passed_with_warning(reason: &'static str) {
    counter!(VALIDATOR_PASSED_WITH_WARNING, "reason" => reason).increment(1);
}

#[inline]
pub fn record_firehose_head(seq: u64) {
    #[expect(clippy::cast_precision_loss)]
    gauge!(FIREHOSE_HEAD).set(seq as f64);
}

#[inline]
pub fn record_queue_depth_bytes(bytes: u64) {
    #[expect(clippy::cast_precision_loss)]
    gauge!(QUEUE_DEPTH_BYTES).set(bytes as f64);
}

#[inline]
pub fn record_discovery_round(outcome: &'static str) {
    counter!(DISCOVERY_ROUND, "outcome" => outcome).increment(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use metrics::with_local_recorder;
    use metrics_exporter_prometheus::PrometheusBuilder;

    #[test]
    fn drop_reason_str_matches_label() {
        assert_eq!(DropReason::CursorMismatch.as_str(), "cursor_mismatch");
        assert_eq!(DropReason::WouldBlock.as_str(), "would_block");
        assert_eq!(DropReason::BufferFull.as_str(), "buffer_full");
        assert_eq!(DropReason::ConsumerTooSlow.as_str(), "consumer_too_slow");
    }

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
    fn record_frame_emitted_increments_labelled_counter() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        with_local_recorder(&recorder, || {
            describe();
            record_frame_emitted("commit", 1);
            record_frame_emitted("commit", 1);
            record_frame_emitted("identity", 1);
        });
        let out = handle.render();
        assert!(out.contains(FRAMES_EMITTED), "missing metric: {out}");
        assert!(out.contains("type=\"commit\""));
        assert!(out.contains("type=\"identity\""));
    }

    #[test]
    fn record_frame_dropped_uses_reason_label() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        with_local_recorder(&recorder, || {
            describe();
            record_frame_dropped(DropReason::CursorMismatch);
            record_frame_dropped(DropReason::ConsumerTooSlow);
        });
        let out = handle.render();
        assert!(out.contains("reason=\"cursor_mismatch\""));
        assert!(out.contains("reason=\"consumer_too_slow\""));
    }

    #[test]
    fn subscriber_count_gauge_round_trips() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        with_local_recorder(&recorder, || {
            describe();
            record_subscriber_count(7);
        });
        let out = handle.render();
        assert!(out.contains(SUBSCRIBER_COUNT));
        assert!(out.contains(" 7"), "expected gauge value 7 in: {out}");
    }

    #[test]
    fn subscriber_lag_histogram_records() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        with_local_recorder(&recorder, || {
            describe();
            record_subscriber_lag_seconds("1.2.3.4:5678", 0.5);
            record_subscriber_lag_seconds("1.2.3.4:5678", 1.5);
        });
        let out = handle.render();
        assert!(out.contains(SUBSCRIBER_LAG_SECONDS));
        assert!(out.contains("addr=\"1.2.3.4:5678\""));
    }

    #[test]
    fn upstream_event_and_disconnect_counters() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        with_local_recorder(&recorder, || {
            describe();
            record_upstream_event("pds.example", "commit");
            record_upstream_disconnect("pds.example", "timeout");
        });
        let out = handle.render();
        assert!(out.contains("host=\"pds.example\""));
        assert!(out.contains("type=\"commit\""));
        assert!(out.contains("reason=\"timeout\""));
    }

    #[test]
    fn validator_rejected_counter() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        with_local_recorder(&recorder, || {
            describe();
            record_validator_rejected("wrong_host");
        });
        let out = handle.render();
        assert!(out.contains(VALIDATOR_REJECTED));
        assert!(out.contains("reason=\"wrong_host\""));
    }

    #[test]
    fn validator_published_deferred_dropped_counters() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        with_local_recorder(&recorder, || {
            describe();
            record_validator_published("commit");
            record_validator_deferred("resolver_pending");
            record_validator_dropped("sig_fail");
        });
        let out = handle.render();
        assert!(out.contains(VALIDATOR_PUBLISHED));
        assert!(out.contains(VALIDATOR_DEFERRED));
        assert!(out.contains(VALIDATOR_DROPPED));
        assert!(out.contains("reason=\"resolver_pending\""));
        assert!(out.contains("reason=\"sig_fail\""));
    }

    #[test]
    fn validator_passed_with_warning_counter() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        with_local_recorder(&recorder, || {
            describe();
            record_validator_passed_with_warning("resolver_pending");
            record_validator_passed_with_warning("pds_mismatch");
            record_validator_passed_with_warning("sig_fail");
            record_validator_passed_with_warning("mst_fail");
        });
        let out = handle.render();
        assert!(out.contains(VALIDATOR_PASSED_WITH_WARNING));
        for r in ["resolver_pending", "pds_mismatch", "sig_fail", "mst_fail"] {
            assert!(out.contains(&format!("reason=\"{r}\"")), "{out} missing {r}");
        }
    }

    #[test]
    fn firehose_head_and_queue_depth_gauges() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        with_local_recorder(&recorder, || {
            describe();
            record_firehose_head(123_456);
            record_queue_depth_bytes(987_654_321);
        });
        let out = handle.render();
        assert!(out.contains(FIREHOSE_HEAD));
        assert!(out.contains(QUEUE_DEPTH_BYTES));
    }

    #[test]
    fn discovery_round_counter() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        with_local_recorder(&recorder, || {
            describe();
            record_discovery_round("ok");
            record_discovery_round("partial");
            record_discovery_round("fail");
        });
        let out = handle.render();
        assert!(out.contains(DISCOVERY_ROUND));
        for o in ["ok", "partial", "fail"] {
            assert!(out.contains(&format!("outcome=\"{o}\"")));
        }
    }

    #[test]
    fn install_recorder_returns_handle_and_tolerates_repeats() {
        // Calling twice in the same process must not panic; second set_global_recorder is dropped.
        let h1 = install_recorder().expect("install");
        let h2 = install_recorder().expect("install");
        drop(h1.render());
        drop(h2.render());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn install_listener_binds_to_kernel_assigned_port() {
        // 127.0.0.1:0 lets the kernel pick a free port; no contention between parallel tests.
        let Ok(addr) = "127.0.0.1:0".parse::<SocketAddr>() else {
            panic!("static addr must parse")
        };
        let result = install_listener(addr);
        assert!(result.is_ok(), "install_listener failed: {:?}", result.err());
    }
}
