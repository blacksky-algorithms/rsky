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
}

/// Build a recorder + handle without binding a socket. Useful for embedding or tests.
pub fn install_recorder() -> Result<PrometheusHandle, MetricsError> {
    let handle = PrometheusBuilder::new().install_recorder()?;
    describe();
    Ok(handle)
}

/// Bind an HTTP /metrics listener and install the recorder globally. Returns the handle.
pub fn install_listener(addr: SocketAddr) -> Result<PrometheusHandle, MetricsError> {
    let (recorder, _exporter) = PrometheusBuilder::new().with_http_listener(addr).build()?;
    let handle = recorder.handle();
    drop(metrics::set_global_recorder(recorder));
    describe();
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
