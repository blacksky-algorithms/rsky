#![deny(
    deprecated_safe,
    future_incompatible,
    let_underscore,
    keyword_idents,
    nonstandard_style,
    refining_impl_trait,
    rust_2018_compatibility,
    rust_2018_idioms,
    rust_2021_compatibility,
    rust_2024_compatibility,
    unused,
    warnings,
    clippy::all,
    clippy::cargo,
    clippy::dbg_macro,
    clippy::expect_used,
    clippy::iter_over_hash_type,
    clippy::nursery,
    clippy::pathbuf_init_then_push,
    clippy::pedantic,
    clippy::print_stderr,
    clippy::print_stdout,
    clippy::renamed_function_params,
    clippy::str_to_string,
    clippy::unused_result_ok,
    clippy::unwrap_used
)]
#![allow(
    clippy::cargo_common_metadata,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::missing_safety_doc,
    clippy::multiple_crate_versions
)]
#![cfg_attr(
    test,
    allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::redundant_pub_crate,
        clippy::min_ident_chars,
        clippy::str_to_string,
        clippy::tests_outside_test_module,
        clippy::let_underscore_must_use,
        clippy::print_stdout,
        clippy::panic,
        clippy::panic_in_result_fn,
        clippy::missing_assert_message,
        clippy::literal_string_with_formatting_args,
        clippy::unused_result_ok,
        clippy::renamed_function_params
    )
)]

mod crawler;
mod publisher;
mod server;
mod types;
mod validator;

#[cfg(feature = "compare")]
pub mod compare;
pub mod config;
pub mod health;
pub mod metrics;

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU8, AtomicU64, Ordering};

use thiserror::Error;

pub static SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// Validator liveness as seen by `/_health`: `Starting` until state load and the
/// legacy reconcile finish, `Live` while the loop runs, `Dead` once it exits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Health {
    Starting = 0,
    Live = 1,
    Dead = 2,
}

impl Health {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Live => "live",
            Self::Dead => "dead",
        }
    }

    #[must_use]
    pub const fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Live,
            2 => Self::Dead,
            _ => Self::Starting,
        }
    }
}

static HEALTH: AtomicU8 = AtomicU8::new(Health::Starting as u8);
/// Unix seconds when the validator last advanced the firehose head or found the ring empty.
pub static HEAD_PROGRESS_AT: AtomicU64 = AtomicU64::new(0);
/// Upstream timestamp (unix seconds) of the newest published event.
pub static HEAD_TIME: AtomicI64 = AtomicI64::new(0);
/// Highest published relay seq.
pub static HEAD_SEQ: AtomicU64 = AtomicU64::new(0);
pub static VALIDATOR_LOOPS: AtomicU64 = AtomicU64::new(0);

pub fn set_health(health: Health) {
    HEALTH.store(health as u8, Ordering::Relaxed);
}

#[must_use]
pub fn health() -> Health {
    Health::from_u8(HEALTH.load(Ordering::Relaxed))
}

#[must_use]
pub fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Progress must have been observed within `stale_after` for `Live` to hold.
#[must_use]
pub fn health_snapshot(stale_after: u64) -> (Health, bool) {
    let health = health();
    let progressed_at = HEAD_PROGRESS_AT.load(Ordering::Relaxed);
    let stalled = health == Health::Live && unix_now().saturating_sub(progressed_at) > stale_after;
    (health, stalled)
}

pub use crawler::Manager as CrawlerManager;
pub use publisher::Manager as PublisherManager;
pub use server::Server;
pub use types::{MessageRecycle, open_source_keyspace};

/// The global keyspace, opened lazily at `RELAY_DB_PATH`.
#[must_use]
pub fn db() -> fjall::Keyspace {
    types::DB.clone()
}
pub use validator::Manager as ValidatorManager;
pub use validator::migrate;

#[derive(Debug, Error)]
pub enum RelayError {
    #[error("crawler error: {0}")]
    Crawler(#[from] crawler::ManagerError),
    #[error("publisher error: {0}")]
    Publisher(#[from] publisher::ManagerError),
    #[error("validator error: {0}")]
    Validator(#[from] validator::ManagerError),
    #[error("server error: {0}")]
    Server(#[from] server::ServerError),
}

/// `SHUTDOWN` is process-global: tests that flip it must not overlap.
#[cfg(test)]
pub(crate) fn shutdown_guard() -> std::sync::MutexGuard<'static, ()> {
    static GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GUARD.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_returns_the_global_keyspace() {
        let tmp = tempfile::tempdir().unwrap();
        // SAFETY: the LazyLock reads the variable once; another test may already have
        // initialised it, in which case this only exercises the accessor.
        unsafe {
            std::env::set_var("RELAY_DB_PATH", tmp.path());
        }
        let _ks = db();
    }

    #[test]
    fn health_round_trips_and_snapshot_detects_stalls() {
        assert_eq!(Health::from_u8(0), Health::Starting);
        assert_eq!(Health::from_u8(1), Health::Live);
        assert_eq!(Health::from_u8(2), Health::Dead);
        assert_eq!(Health::from_u8(99), Health::Starting);
        for h in [Health::Starting, Health::Live, Health::Dead] {
            assert_eq!(Health::from_u8(h as u8), h);
            assert!(!h.as_str().is_empty());
        }
        assert!(unix_now() > 1_700_000_000);
        set_health(Health::Live);
        HEAD_PROGRESS_AT.store(unix_now(), Ordering::Relaxed);
        assert_eq!(health_snapshot(60), (Health::Live, false));
        HEAD_PROGRESS_AT.store(unix_now() - 120, Ordering::Relaxed);
        assert_eq!(health_snapshot(60), (Health::Live, true));
        set_health(Health::Dead);
        assert_eq!(health_snapshot(60), (Health::Dead, false));
        set_health(Health::Starting);
    }
}
