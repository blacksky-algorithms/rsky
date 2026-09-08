//! Per-PDS-host budgets: hostname normalisation, host classes, rate limits and
//! adaptive concurrency.
//!
//! Derived from hubble's `car-dump` fetcher (`hubble-pds::Hostname` and
//! `dump_repos::HostHealth`), which has backfilled the whole network with this
//! shape. The network splits into two classes that want different treatment:
//!
//! * Bluesky's mushroom fleet (`*.host.bsky.network`), ~90 hosts carrying ~98%
//!   of repos. High capacity, a documented per-IP budget (the PDS global
//!   limiter is 3000 requests / 5 min = 10 req/s), transient errors that clear
//!   quickly. We run them at a fixed rate and never throttle down.
//! * Everyone else: ~6,000 hosts, most tiny, many on residential links. Low
//!   fixed rate, and concurrency that steps down under transient errors and
//!   creeps back up when the host recovers.

use std::num::NonZeroU32;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::source::{COOLDOWN_FLOOR_SECS, COOLDOWN_MAX_SECS, Class};

/// A normalised PDS hostname: lowercase, no port, no trailing dot.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hostname(String);

#[derive(Debug, thiserror::Error)]
#[error("invalid hostname: {0:?}")]
pub struct HostnameError(String);

impl Hostname {
    pub fn new(raw: &str) -> Result<Self, HostnameError> {
        let lower = raw.trim().to_lowercase();
        let no_scheme = lower
            .strip_prefix("https://")
            .or_else(|| lower.strip_prefix("http://"))
            .unwrap_or(&lower);
        let no_path = no_scheme.split('/').next().unwrap_or("");
        let no_port = no_path.split(':').next().unwrap_or("");
        let trimmed = no_port.trim_end_matches('.');
        if trimmed.is_empty() || trimmed.contains(char::is_whitespace) {
            return Err(HostnameError(raw.to_owned()));
        }
        Ok(Self(trimmed.to_owned()))
    }

    #[must_use]
    // `String: Deref` is not const on this toolchain, whatever clippy thinks.
    #[allow(clippy::missing_const_for_fn)]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn base_url(&self) -> String {
        format!("https://{}", self.0)
    }

    /// Bluesky's PDS fleet.
    #[must_use]
    pub fn is_bsky(&self) -> bool {
        self.0.ends_with(".host.bsky.network")
    }

    /// Bridgy Fed. It bridges tens of thousands of accounts and does not
    /// reliably serve `getRepo`; pinned to 1 req/s regardless of class.
    #[must_use]
    pub fn is_bridgy(&self) -> bool {
        self.0.ends_with(".brid.gy")
    }
}

impl std::fmt::Display for Hostname {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::str::FromStr for Hostname {
    type Err = HostnameError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

/// Budget for one host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostLimits {
    /// Self-imposed request rate, enforced by a token bucket.
    pub rps: NonZeroU32,
    /// In-flight fetch ceiling.
    pub concurrency: usize,
    /// Whether concurrency steps down under transient errors. Off for the
    /// mushrooms: their transient errors say nothing about our budget.
    pub adaptive: bool,
}

const fn nz(v: u32) -> NonZeroU32 {
    match NonZeroU32::new(v) {
        Some(n) => n,
        None => NonZeroU32::MIN,
    }
}

/// Which hosts we fetch from directly, and at what budget. Everything not
/// matched by `direct_patterns` is left to hubble.
#[derive(Debug, Clone)]
pub struct HostPolicy {
    pub bsky: HostLimits,
    pub generic: HostLimits,
    /// Suffix globs (`*.host.bsky.network`) or exact hostnames.
    pub direct_patterns: Vec<String>,
}

impl Default for HostPolicy {
    fn default() -> Self {
        Self {
            // The PDS global limiter is 3000 req / 5 min per IP. Run at it,
            // not above it; measured 429s are the signal to lower this.
            bsky: HostLimits {
                rps: nz(10),
                concurrency: 10,
                adaptive: false,
            },
            // car-dump's defaults for the long tail.
            generic: HostLimits {
                rps: nz(3),
                concurrency: 6,
                adaptive: true,
            },
            direct_patterns: vec!["*.host.bsky.network".to_owned()],
        }
    }
}

impl HostPolicy {
    #[must_use]
    pub fn limits_for(&self, host: &Hostname) -> HostLimits {
        let mut limits = if host.is_bsky() {
            self.bsky
        } else {
            self.generic
        };
        if host.is_bridgy() {
            limits.rps = nz(1);
            limits.concurrency = 1;
        }
        limits
    }

    /// Whether this host is fetched directly rather than through hubble.
    #[must_use]
    pub fn is_direct(&self, host: &Hostname) -> bool {
        self.direct_patterns
            .iter()
            .any(|p| pattern_matches(p, host.as_str()))
    }
}

/// `*.suffix` matches any host ending in `.suffix`; anything else is exact.
#[must_use]
pub fn pattern_matches(pattern: &str, host: &str) -> bool {
    let pattern = pattern.trim().to_lowercase();
    pattern.strip_prefix('*').map_or_else(
        || pattern == host,
        |suffix| host.ends_with(suffix) && host.len() > suffix.len(),
    )
}

/// How many consecutive transient outcomes step a host's concurrency down.
/// Fast-failing repos bunch up in completion order ahead of slow successes, so
/// a short burst must not clamp a healthy host.
const REDUCE_TRANSIENT_THRESHOLD: u32 = 3;
/// Each step keeps ~3/4 of the previous limit: 10 → 7 → 5 → 3 → 2 → 1.
const REDUCE_NUMER: usize = 3;
const REDUCE_DENOM: usize = 4;
/// Once a host has gone this long without a reduction, recover one unit.
pub const RECOVER_AFTER: Duration = Duration::from_secs(240);

/// Adaptive per-host health, shared across a host's in-flight fetches.
///
/// One mutex rather than atomics: the streak→reduce transition must be
/// race-free or two concurrent errors could reduce twice in one streak.
#[derive(Debug)]
pub struct HostHealth {
    inner: Mutex<HealthInner>,
    adaptive: bool,
    max_limit: usize,
}

#[derive(Debug)]
struct HealthInner {
    consecutive_transient: u32,
    limit: usize,
    triggered: bool,
    max_retry_after_secs: u64,
    last_change: Instant,
}

impl HostHealth {
    #[must_use]
    pub fn new(initial_limit: usize, max_limit: usize, adaptive: bool) -> Self {
        Self {
            inner: Mutex::new(HealthInner {
                consecutive_transient: 0,
                limit: initial_limit.clamp(1, max_limit.max(1)),
                triggered: false,
                max_retry_after_secs: 0,
                last_change: Instant::now(),
            }),
            adaptive,
            max_limit: max_limit.max(1),
        }
    }

    #[must_use]
    pub fn from_limits(limits: HostLimits, stored_limit: Option<usize>) -> Self {
        let initial = if limits.adaptive {
            stored_limit.unwrap_or(limits.concurrency)
        } else {
            limits.concurrency
        };
        Self::new(initial, limits.concurrency, limits.adaptive)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HealthInner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Current in-flight ceiling.
    #[must_use]
    pub fn limit(&self) -> usize {
        self.lock().limit
    }

    /// Whether the floor was exhausted; sticky until the host is rebuilt after
    /// its cooldown.
    #[must_use]
    pub fn triggered(&self) -> bool {
        self.lock().triggered
    }

    /// Record one outcome. `None` is success. Returns the new limit when this
    /// outcome completed a transient streak and reduced concurrency.
    pub fn record(&self, failure: Option<Class>, retry_after_secs: Option<u64>) -> Option<usize> {
        let mut inner = self.lock();
        if let Some(secs) = retry_after_secs {
            inner.max_retry_after_secs =
                inner.max_retry_after_secs.max(secs.min(COOLDOWN_MAX_SECS));
        }
        if !self.adaptive {
            return None;
        }
        match failure {
            None => {
                inner.consecutive_transient = 0;
                None
            }
            Some(class) if class.throttles_host() => {
                inner.consecutive_transient += 1;
                if inner.consecutive_transient < REDUCE_TRANSIENT_THRESHOLD {
                    return None;
                }
                inner.consecutive_transient = 0;
                if inner.limit > 1 {
                    inner.limit = (inner.limit * REDUCE_NUMER / REDUCE_DENOM)
                        .max(1)
                        .min(inner.limit - 1);
                    inner.last_change = Instant::now();
                    Some(inner.limit)
                } else {
                    inner.triggered = true;
                    None
                }
            }
            // Per-repo failures say nothing about the host.
            Some(_) => None,
        }
    }

    /// Raise the limit by one if it has been quiet for [`RECOVER_AFTER`].
    pub fn maybe_recover(&self, now: Instant) -> Option<usize> {
        if !self.adaptive {
            return None;
        }
        let mut inner = self.lock();
        if inner.triggered || inner.limit >= self.max_limit {
            return None;
        }
        if now.duration_since(inner.last_change) < RECOVER_AFTER {
            return None;
        }
        inner.limit += 1;
        inner.last_change = now;
        Some(inner.limit)
    }

    /// Cooldown to apply once triggered: the floor, or the largest
    /// `Retry-After` seen, whichever is greater.
    #[must_use]
    pub fn cooldown_secs(&self) -> u64 {
        self.lock().max_retry_after_secs.max(COOLDOWN_FLOOR_SECS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(s: &str) -> Hostname {
        Hostname::new(s).unwrap()
    }

    #[test]
    fn hostnames_normalise() {
        assert_eq!(h("Example.COM").as_str(), "example.com");
        assert_eq!(h("example.com.").as_str(), "example.com");
        assert_eq!(h("example.com:8443").as_str(), "example.com");
        assert_eq!(h("https://example.com/xrpc").as_str(), "example.com");
        assert!(Hostname::new("").is_err());
        assert!(Hostname::new(":443").is_err());
        assert!(Hostname::new("a b").is_err());
        assert_eq!(h("Example.COM:443.").base_url(), "https://example.com");
    }

    #[test]
    fn host_classes() {
        assert!(h("morel.us-east.host.bsky.network").is_bsky());
        assert!(!h("bsky.network").is_bsky());
        assert!(h("atproto.brid.gy").is_bridgy());
        assert!(!h("blacksky.app").is_bsky());
    }

    #[test]
    fn policy_limits_by_class() {
        let p = HostPolicy::default();
        let bsky = p.limits_for(&h("morel.us-east.host.bsky.network"));
        assert!(!bsky.adaptive);
        assert_eq!(bsky.rps.get(), 10);
        let generic = p.limits_for(&h("blacksky.app"));
        assert!(generic.adaptive);
        assert_eq!(generic.rps.get(), 3);
        let bridgy = p.limits_for(&h("atproto.brid.gy"));
        assert_eq!((bridgy.rps.get(), bridgy.concurrency), (1, 1));
    }

    #[test]
    fn direct_patterns_are_suffix_globs_or_exact() {
        let mut p = HostPolicy::default();
        assert!(p.is_direct(&h("lepista.us-west.host.bsky.network")));
        assert!(!p.is_direct(&h("blacksky.app")));
        assert!(!pattern_matches(
            "*.host.bsky.network",
            ".host.bsky.network"
        ));
        p.direct_patterns.push("blacksky.app".into());
        assert!(p.is_direct(&h("blacksky.app")));
        assert!(!p.is_direct(&h("pds.blacksky.app")));
    }

    #[test]
    fn health_reduces_after_a_streak_and_success_resets() {
        let hh = HostHealth::new(10, 10, true);
        assert_eq!(hh.record(Some(Class::Transport), None), None);
        assert_eq!(hh.record(Some(Class::Server5xx), None), None);
        assert_eq!(hh.record(Some(Class::Server5xx), None), Some(7));
        assert_eq!(hh.limit(), 7);
        assert!(!hh.triggered());
        hh.record(None, None);
        assert_eq!(hh.record(Some(Class::RateLimited), None), None);
        assert_eq!(hh.record(Some(Class::RateLimited), None), None);
        assert_eq!(hh.record(Some(Class::RateLimited), None), Some(5));
    }

    #[test]
    fn health_steps_to_the_floor_then_trips() {
        let hh = HostHealth::new(10, 10, true);
        for expected in [7, 5, 3, 2, 1] {
            assert_eq!(hh.record(Some(Class::Server5xx), None), None);
            assert_eq!(hh.record(Some(Class::Server5xx), None), None);
            assert_eq!(hh.record(Some(Class::Server5xx), None), Some(expected));
        }
        for _ in 0..3 {
            assert_eq!(hh.record(Some(Class::Server5xx), None), None);
        }
        assert!(hh.triggered());
        hh.record(None, None);
        assert!(hh.triggered(), "sticky");
    }

    #[test]
    fn terminal_and_app_errors_do_not_move_health() {
        let hh = HostHealth::new(4, 4, true);
        for _ in 0..8 {
            assert_eq!(hh.record(Some(Class::Terminal), None), None);
            assert_eq!(hh.record(Some(Class::Server5xxApp), None), None);
        }
        assert_eq!(hh.limit(), 4);
        // Neutral outcomes neither reset nor extend a streak in progress.
        assert_eq!(hh.record(Some(Class::Server5xx), None), None);
        assert_eq!(hh.record(Some(Class::Terminal), None), None);
        assert_eq!(hh.record(Some(Class::Server5xx), None), None);
        assert_eq!(hh.record(Some(Class::Server5xx), None), Some(3));
    }

    #[test]
    fn non_adaptive_hosts_never_reduce() {
        let hh = HostHealth::from_limits(
            HostLimits {
                rps: nz(10),
                concurrency: 10,
                adaptive: false,
            },
            Some(2),
        );
        assert_eq!(hh.limit(), 10, "stored reductions are ignored for bsky");
        for _ in 0..20 {
            assert_eq!(hh.record(Some(Class::RateLimited), None), None);
        }
        assert!(!hh.triggered());
        assert_eq!(hh.maybe_recover(Instant::now()), None);
    }

    #[test]
    fn adaptive_hosts_resume_at_their_stored_floor() {
        let hh = HostHealth::from_limits(
            HostLimits {
                rps: nz(3),
                concurrency: 6,
                adaptive: true,
            },
            Some(2),
        );
        assert_eq!(hh.limit(), 2);
        let hh = HostHealth::from_limits(
            HostLimits {
                rps: nz(3),
                concurrency: 6,
                adaptive: true,
            },
            Some(99),
        );
        assert_eq!(hh.limit(), 6, "clamped to the ceiling");
    }

    #[test]
    fn recovery_creeps_up_and_caps() {
        let hh = HostHealth::new(10, 10, true);
        let t0 = Instant::now();
        for _ in 0..2 {
            hh.record(Some(Class::Server5xx), None);
        }
        assert_eq!(hh.record(Some(Class::Server5xx), None), Some(7));
        assert_eq!(hh.maybe_recover(t0 + Duration::from_secs(1)), None);
        let mut t = t0 + RECOVER_AFTER + Duration::from_secs(1);
        assert_eq!(hh.maybe_recover(t), Some(8));
        assert_eq!(hh.maybe_recover(t), None);
        t += RECOVER_AFTER;
        assert_eq!(hh.maybe_recover(t), Some(9));
        t += RECOVER_AFTER;
        assert_eq!(hh.maybe_recover(t), Some(10));
        t += RECOVER_AFTER;
        assert_eq!(hh.maybe_recover(t), None);
    }

    #[test]
    fn cooldown_honours_retry_after_within_bounds() {
        let hh = HostHealth::new(10, 10, true);
        assert_eq!(hh.cooldown_secs(), COOLDOWN_FLOOR_SECS);
        hh.record(Some(Class::RateLimited), Some(2));
        assert_eq!(hh.cooldown_secs(), COOLDOWN_FLOOR_SECS);
        hh.record(Some(Class::RateLimited), Some(45));
        assert_eq!(hh.cooldown_secs(), 45);
        hh.record(Some(Class::RateLimited), Some(99_999_999));
        assert_eq!(hh.cooldown_secs(), COOLDOWN_MAX_SECS);
        // Retry-After is recorded even for non-adaptive hosts.
        let bsky = HostHealth::new(10, 10, false);
        bsky.record(Some(Class::RateLimited), Some(30));
        assert_eq!(bsky.cooldown_secs(), 30);
    }
}
