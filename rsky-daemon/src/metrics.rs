//! Prometheus metrics, served at `GET /metrics` on the notify listener.

use prometheus::{
    register_int_counter_vec_with_registry, register_int_gauge_vec_with_registry, Encoder,
    IntCounterVec, IntGaugeVec, Registry, TextEncoder,
};
use std::sync::LazyLock;

static REGISTRY: LazyLock<Registry> = LazyLock::new(Registry::new);

static SWEEPS: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec_with_registry!(
        "daemon_sweeps_total",
        "Space sweeps by result",
        &["result"],
        REGISTRY
    )
    .expect("register daemon_sweeps_total")
});

static REGISTRATIONS: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec_with_registry!(
        "daemon_notify_registrations_total",
        "Notify registrations by result",
        &["result"],
        REGISTRY
    )
    .expect("register daemon_notify_registrations_total")
});

static NOTIFIED_SYNCS: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec_with_registry!(
        "daemon_notified_syncs_total",
        "Repo syncs triggered by a write notification, by result",
        &["result"],
        REGISTRY
    )
    .expect("register daemon_notified_syncs_total")
});

static SPACE_SWEEP_OK: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec_with_registry!(
        "daemon_space_sweep_ok",
        "1 when the space's most recent sweep succeeded, 0 when it failed",
        &["space"],
        REGISTRY
    )
    .expect("register daemon_space_sweep_ok")
});

static SPACE_NOTIFY_REGISTERED: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec_with_registry!(
        "daemon_space_notify_registered",
        "1 when the space's most recent notify registration succeeded, 0 when it failed",
        &["space"],
        REGISTRY
    )
    .expect("register daemon_space_notify_registered")
});

fn result(ok: bool) -> &'static str {
    if ok {
        "ok"
    } else {
        "error"
    }
}

pub fn record_sweep(space: &str, ok: bool) {
    SWEEPS.with_label_values(&[result(ok)]).inc();
    SPACE_SWEEP_OK.with_label_values(&[space]).set(ok as i64);
}

pub fn record_registration(space: &str, ok: bool) {
    REGISTRATIONS.with_label_values(&[result(ok)]).inc();
    SPACE_NOTIFY_REGISTERED
        .with_label_values(&[space])
        .set(ok as i64);
}

pub fn record_notified_sync(ok: bool) {
    NOTIFIED_SYNCS.with_label_values(&[result(ok)]).inc();
}

/// Drop a space's gauges once its worker stops, so a removed space does not
/// keep reporting its last state.
pub fn forget_space(space: &str) {
    let _ = SPACE_SWEEP_OK.remove_label_values(&[space]);
    let _ = SPACE_NOTIFY_REGISTERED.remove_label_values(&[space]);
}

pub fn encode() -> String {
    LazyLock::force(&SWEEPS);
    LazyLock::force(&REGISTRATIONS);
    LazyLock::force(&NOTIFIED_SYNCS);
    LazyLock::force(&SPACE_SWEEP_OK);
    LazyLock::force(&SPACE_NOTIFY_REGISTERED);
    let mut buf = Vec::new();
    TextEncoder::new()
        .encode(&REGISTRY.gather(), &mut buf)
        .expect("text encoding of gathered metrics");
    String::from_utf8(buf).expect("prometheus text format is UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_gauges_track_the_latest_result_and_are_forgotten() {
        let space = "at://did:plc:metrics-test/space/community.blacksky.feed/a";
        record_sweep(space, true);
        record_registration(space, true);
        record_sweep(space, false);
        record_notified_sync(false);
        let text = encode();
        assert!(text.contains(&format!("daemon_space_sweep_ok{{space=\"{space}\"}} 0")));
        assert!(text.contains(&format!(
            "daemon_space_notify_registered{{space=\"{space}\"}} 1"
        )));
        assert!(text.contains("daemon_sweeps_total{result=\"error\"}"));
        assert!(text.contains("daemon_notified_syncs_total{result=\"error\"}"));

        forget_space(space);
        assert!(!encode().contains(space));
    }
}
