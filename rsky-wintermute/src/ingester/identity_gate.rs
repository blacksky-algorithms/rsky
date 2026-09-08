//! Bounds the per-event tasks the firehose reader spawns for `#identity`,
//! `#account` and `#sync` events.
//!
//! Those tasks were untracked `tokio::spawn`s with no concurrency limit and no
//! deadline. Each one builds an [`IdResolver`] (two reqwest clients with a
//! fresh TLS config) and resolves a DID and a handle; the handle resolver
//! constructs a DNS resolver per call with no timeout of its own. During a
//! relay replay or a PDS migration wave, thousands of them can be in flight,
//! each holding its stack, its buffers and a pooled Postgres connection.
//!
//! The gate mirrors what `labels.rs` already does for label events: a
//! semaphore sized by `IDENTITY_EVENT_CONCURRENCY`, plus a per-task deadline
//! (`IDENTITY_EVENT_TIMEOUT_SECS`) so a stalled resolver cannot pin a permit.
//! Unlike the labels path the reader never waits for a permit: identity events
//! are advisory (the handle-resolution sweep in the indexer re-verifies every
//! handle within a day), so under sustained overload the event is shed and
//! counted rather than stalling the firehose behind DNS.

use std::future::Future;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use prometheus::{IntCounterVec, IntGauge, register_int_counter_vec, register_int_gauge};
use rsky_identity::IdResolver;
use rsky_identity::types::IdentityResolverOpts;
use tokio::sync::Semaphore;

use crate::config::{IDENTITY_EVENT_CONCURRENCY, IDENTITY_EVENT_TIMEOUT};

pub static INGESTER_IDENTITY_TASKS_IN_FLIGHT: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "ingester_identity_tasks_in_flight",
        "Identity/account/sync event tasks currently running (bounded by IDENTITY_EVENT_CONCURRENCY)"
    )
    .unwrap_or_else(|e| panic!("ingester_identity_tasks_in_flight: {e}"))
});

pub static INGESTER_IDENTITY_TASK_TIMEOUTS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        "ingester_identity_task_timeouts_total",
        "Identity/account/sync event tasks abandoned at IDENTITY_EVENT_TIMEOUT_SECS, by event kind",
        &["kind"]
    )
    .unwrap_or_else(|e| panic!("ingester_identity_task_timeouts_total: {e}"))
});

pub static INGESTER_IDENTITY_TASKS_SHED_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        "ingester_identity_tasks_shed_total",
        "Identity/account/sync events dropped because every permit was busy, by event kind",
        &["kind"]
    )
    .unwrap_or_else(|e| panic!("ingester_identity_tasks_shed_total: {e}"))
});

/// The process-wide gate, shared by every relay connection.
pub static IDENTITY_GATE: LazyLock<IdentityGate> =
    LazyLock::new(|| IdentityGate::new(*IDENTITY_EVENT_CONCURRENCY, *IDENTITY_EVENT_TIMEOUT));

/// One resolver for the whole ingester. `IdResolver::new` builds two reqwest
/// clients (PLC and did:web) each carrying a rustls config; cloning the shared
/// one is an `Arc` bump. Its DID cache is constructed with zero TTLs, so
/// sharing it does not introduce a growing cache.
static SHARED_RESOLVER: LazyLock<IdResolver> = LazyLock::new(|| {
    IdResolver::new(IdentityResolverOpts {
        timeout: Some(Duration::from_secs(5)),
        plc_url: None,
        did_cache: None,
        backup_nameservers: None,
    })
});

/// A clone of the ingester's shared identity resolver.
#[must_use]
pub fn resolver() -> IdResolver {
    SHARED_RESOLVER.clone()
}

pub struct IdentityGate {
    permits: Arc<Semaphore>,
    concurrency: usize,
    timeout: Duration,
}

impl IdentityGate {
    #[must_use]
    pub fn new(concurrency: usize, timeout: Duration) -> Self {
        let concurrency = concurrency.max(1);
        Self {
            permits: Arc::new(Semaphore::new(concurrency)),
            concurrency,
            timeout,
        }
    }

    /// Run `work` on the current runtime if a permit is free; otherwise shed
    /// it. Returns whether the task was spawned. `kind` labels the counters.
    pub fn spawn<F>(&self, kind: &'static str, work: F) -> bool
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let Ok(permit) = Arc::clone(&self.permits).try_acquire_owned() else {
            tracing::warn!(
                "shedding {kind} event: all {} identity permits busy",
                self.concurrency
            );
            INGESTER_IDENTITY_TASKS_SHED_TOTAL
                .with_label_values(&[kind])
                .inc();
            return false;
        };

        INGESTER_IDENTITY_TASKS_IN_FLIGHT.inc();
        let timeout = self.timeout;
        drop(tokio::spawn(async move {
            if tokio::time::timeout(timeout, work).await.is_err() {
                tracing::warn!(
                    "{kind} event task exceeded {}s, abandoned",
                    timeout.as_secs()
                );
                INGESTER_IDENTITY_TASK_TIMEOUTS_TOTAL
                    .with_label_values(&[kind])
                    .inc();
            }
            INGESTER_IDENTITY_TASKS_IN_FLIGHT.dec();
            drop(permit);
        }));
        true
    }

    /// Permits not currently held.
    #[must_use]
    pub fn available(&self) -> usize {
        self.permits.available_permits()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    async fn settle() {
        // Let spawned tasks run to their next await point.
        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test]
    async fn releases_the_permit_when_work_completes() {
        let gate = IdentityGate::new(2, Duration::from_secs(5));
        let ran = Arc::new(AtomicUsize::new(0));
        let r = Arc::clone(&ran);
        assert!(gate.spawn("identity", async move {
            r.fetch_add(1, Ordering::SeqCst);
        }));
        settle().await;
        assert_eq!(ran.load(Ordering::SeqCst), 1);
        assert_eq!(gate.available(), 2, "permit must return after completion");
    }

    #[tokio::test]
    async fn sheds_when_every_permit_is_busy() {
        let gate = IdentityGate::new(1, Duration::from_secs(5));
        let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
        let before = INGESTER_IDENTITY_TASKS_SHED_TOTAL
            .with_label_values(&["account"])
            .get();

        assert!(gate.spawn("account", async move {
            drop(release_rx.await);
        }));
        assert!(
            !gate.spawn("account", async {}),
            "second task must be shed while the only permit is held"
        );
        assert_eq!(
            INGESTER_IDENTITY_TASKS_SHED_TOTAL
                .with_label_values(&["account"])
                .get(),
            before + 1
        );

        assert!(release_tx.send(()).is_ok(), "task is still waiting");
        settle().await;
        assert_eq!(gate.available(), 1);
        assert!(
            gate.spawn("account", async {}),
            "permit is reusable once freed"
        );
    }

    #[tokio::test]
    async fn abandons_work_at_the_deadline_and_counts_it() {
        let gate = IdentityGate::new(1, Duration::from_millis(20));
        let before = INGESTER_IDENTITY_TASK_TIMEOUTS_TOTAL
            .with_label_values(&["sync"])
            .get();

        assert!(gate.spawn("sync", async {
            tokio::time::sleep(Duration::from_secs(60)).await;
        }));
        settle().await;
        assert_eq!(gate.available(), 0, "permit held while the task runs");

        // Poll rather than sleep a fixed amount so a slow CI box cannot flake this.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while gate.available() == 0 && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        assert_eq!(
            INGESTER_IDENTITY_TASK_TIMEOUTS_TOTAL
                .with_label_values(&["sync"])
                .get(),
            before + 1
        );
        assert_eq!(gate.available(), 1, "permit must return after a timeout");
    }

    #[test]
    fn zero_concurrency_is_clamped_to_one() {
        let gate = IdentityGate::new(0, Duration::from_secs(1));
        assert_eq!(gate.available(), 1);
    }

    #[test]
    fn shared_resolver_clones_cheaply_and_consistently() {
        let a = resolver();
        let b = resolver();
        assert_eq!(a.handle.timeout, b.handle.timeout);
        assert_eq!(a.handle.timeout, Duration::from_secs(5));
    }
}
