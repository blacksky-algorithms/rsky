//! Public external hosts -- upstream PDSes and subscribeRepos sources
//!
//! interned, owned limiters, throttled xrpc helpers
//!
//! `Host` carries the hostname and the resources (lightweight: limiter,
//! connection semaphore, sync1.1 ratchet, request client ref) relevant to per-
//! host upstream work. interned to ensure every repo or scheduler task on the
//! same host is working with the same `Arc<Host>`.
//!
//! the `Arc<Host>` refcount determines the lifetime -- the host registry just
//! caches a `Weak<Host>`, so it can fully drop when no longer referenced. the
//! registry still retains keys of dropped hosts until `.prune()` is called.
//! TODO: if it's cheap, maybe we should have a per-host cleanup api for actors
//! to call when unloading?
//!
//! interning doesn't do i/o -- the sync1.1 ratchet is an uninitialized OnceLock
//! until needed. (...should we do this with the limits? they might become per-
//! host configurable in the database. maybe overrides are kept in mem or smth)

mod client;
mod hostname;
mod registry;
mod xrpc;

pub(crate) use client::{HostClientError, LimitWaited, RedirectingClient, ResolvedHost};
pub use hostname::{Hostname, HostnameError, serialize_name};
pub use registry::{HostRegistry, HostRegistryConfig};
pub use xrpc::{HostRequestError, ListHostsResponse, PdsHost};

use hostname::normalize_hostname;

use std::fmt;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use governor::{Quota, RateLimiter};
use jacquard_common::deps::fluent_uri::Uri as FluentUri;
use metrics::counter;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tracing::{debug, info};

use crate::metrics::{HOST_BACKOFFS_TOTAL, HOST_SYNC_COMPLIANCE_UPGRADES_TOTAL};
use crate::storage::host_info;
use crate::{StorageBatch, StorageEngine, StorageError, SyncScope};

const HOST_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const HOST_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

const DEFAULT_RATELIMIT_BACKOFF: Duration = Duration::from_secs(60);
const DEFAULT_TRANSIENT_BACKOFF: Duration = Duration::from_secs(30);
const DEFAULT_MAX_BACKOFF: Duration = Duration::from_secs(900);

/// the maximum size we'll read to try to decode a non-ok response
///
/// TODO: check if jacquard imposes/exposes any response body size limits
const MAX_ERROR_BODY: usize = 8 * 1024;

const DEFAULT_HOST_QPS: NonZeroU32 = NonZeroU32::new(2).unwrap(); // TODO: config/policy
const DEFAULT_BSKY_HOST_QPS: NonZeroU32 = NonZeroU32::new(10).unwrap(); // bsky hasn't bumped getRepo to 20req/s yet?
const DEFAULT_RELAY_HOST_QPS: NonZeroU32 = NonZeroU32::new(1000).unwrap();

const DEFAULT_HOST_CONCURRENCY: usize = 2; // TODO: config / policy-based; be kind to hosts for now
const DEFAULT_BSKY_HOST_CONCURRENCY: usize = 10; // ouch, we can't get much better than this from bsky haproxy, so long-rtt backfills are slow
const DEFAULT_RELAY_HOST_CONCURRENCY: usize = 128;

const DEFAULT_MAX_REDIRECTS: usize = 10;
const DEFAULT_MAX_NON_STREAM_BODY: usize = 10 * 2_usize.pow(20); // 10MiB

#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("host backing off, {remaining:?} left")]
pub struct InBackoff {
    pub remaining: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Sync11State {
    // host might not be emitting sync1.1
    Lax = 0,
    // host has done a sync-1.1-looking thing. strict never becomes lax again.
    Strict = 1,
}

impl Sync11State {
    fn from_u8(b: u8) -> Self {
        match b {
            0 => Self::Lax,
            1 => Self::Strict,
            _ => unreachable!("impossible sync1.1 state"),
        }
    }
    fn as_u8(&self) -> u8 {
        *self as u8
    }
}

/// for the sync1.1 ratchet. bleh.
#[derive(Debug)]
pub(crate) struct ComplianceCell(AtomicU8);

impl ComplianceCell {
    const UNSET: u8 = 0xFF;
    pub fn new_unset() -> Self {
        Self(AtomicU8::new(Self::UNSET))
    }
    pub fn get(&self) -> Option<Sync11State> {
        match self.0.load(Ordering::Acquire) {
            Self::UNSET => None,
            b => Some(Sync11State::from_u8(b)),
        }
    }
    pub fn set_if_unset(&self, s: Sync11State) -> Sync11State {
        match self
            .0
            .compare_exchange(Self::UNSET, s.as_u8(), Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => s,
            Err(other) => Sync11State::from_u8(other),
        }
    }
    pub fn upgrade(&self) -> bool {
        let strict = Sync11State::Strict.as_u8();
        let prev = self.0.swap(strict, Ordering::AcqRel);
        prev != strict
    }
}

/// host-wide ratelimit/transient-error backoff
///
/// unix millis "wait-until" marker. if future, wait until then before going
/// ahead with a request. only ever advances. initializes with 0 (always past)
#[derive(Debug)]
pub(crate) struct BackoffCell(AtomicU64);

impl BackoffCell {
    pub fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    pub fn push_to(&self, then: SystemTime, now: SystemTime) {
        let unix_ms = then
            .min(now + DEFAULT_MAX_BACKOFF)
            .duration_since(UNIX_EPOCH)
            .expect("deadline after epoch")
            .as_millis() as u64;
        self.0.fetch_max(unix_ms, Ordering::AcqRel);
    }

    pub fn backoff(&self, now: SystemTime) -> Option<Duration> {
        let unix_ms = self.0.load(Ordering::Acquire);
        let then = UNIX_EPOCH + Duration::from_millis(unix_ms);
        then.duration_since(now).ok()
    }
}

/// a remote host, usually a PDS or subscribeRepos upstream
///
/// hosts in hubble-sync hosts are interned in a registry, so multiple handles
/// to the same host use the same underlying resource, including self-rate-
/// limiting and request concurrency limits.
///
/// we should almost always hold on to whole `Host`s, not `Hostnames`. ff we're
/// talking about a host somewhere then it's useful to keep it alive. in
/// particular, the norm is to pass `&Host` or `Arc<Host>` to functions, and
/// hold `Arc<Host`s on owned structs; the hostname is accessible at point of
/// use via `host.name()`.
pub struct Host {
    name: Hostname,
    client: Arc<RedirectingClient>,
    limiter: governor::DefaultDirectRateLimiter,
    request_concurrency: Arc<Semaphore>,
    backoff: BackoffCell,
    sync_compliance: ComplianceCell,
    in_scope: bool, // temp: (maybe) host-based sync policy
}

impl Host {
    /// internal constructor (use the host registry)
    fn new(
        name: Hostname,
        client: Arc<RedirectingClient>,
        qps: NonZeroU32,
        concurrency: usize,
        sync_scope: SyncScope,
    ) -> Self {
        let mut me = Self {
            name,
            client,
            limiter: RateLimiter::direct(Quota::per_second(qps)),
            request_concurrency: Arc::new(Semaphore::new(concurrency)),
            backoff: BackoffCell::new(),
            sync_compliance: ComplianceCell::new_unset(),
            in_scope: true,
        };
        if !sync_scope.allows_host(&me) {
            // slightly awk, oh well
            me.in_scope = false;
        }
        me
    }

    pub fn name(&self) -> &Hostname {
        &self.name
    }

    pub fn in_scope(&self) -> bool {
        self.in_scope
    }

    pub fn jacquard_uri_base(&self, scheme: &str) -> Result<FluentUri<String>, String> {
        // ...could maybe use the builder stuff, but it's pretty verbose
        let base = format!("{scheme}://{}", self.name().as_str());
        let uri = FluentUri::parse(base).map_err(|e| format!("failed to parse uri: {e:?}"))?;
        Ok(uri)
    }

    /// apply backoff for 429s or transient 500s passing through
    pub(crate) fn observe_response(&self, status: http::StatusCode, headers: &http::HeaderMap) {
        use http::StatusCode as S;
        let (fallback_backoff, label) = match status {
            S::TOO_MANY_REQUESTS => (DEFAULT_RATELIMIT_BACKOFF, "429"),
            S::BAD_GATEWAY => (DEFAULT_TRANSIENT_BACKOFF, "502"),
            S::SERVICE_UNAVAILABLE => (DEFAULT_TRANSIENT_BACKOFF, "503"),
            S::GATEWAY_TIMEOUT => (DEFAULT_TRANSIENT_BACKOFF, "504"),
            _ => return,
        };
        let now = SystemTime::now();
        let until = parse_retry_after(headers, now).unwrap_or(now + fallback_backoff);
        info!(
            host = %self.name,
            %status,
            backoff = ?until.duration_since(now),
            "backing off requests (actual backoff may be capped)",
        );
        counter!(HOST_BACKOFFS_TOTAL, "status" => label).increment(1);
        self.backoff.push_to(until, now);
    }

    /// how long (if any) until this host is out of backoff
    ///
    /// backoff happens when we get a rate-limit 429 or transient 502/503/504
    pub fn backoff_remaining(&self, now: SystemTime) -> Option<Duration> {
        self.backoff.backoff(now)
    }

    pub async fn request_permit(&self) -> Result<OwnedSemaphorePermit, InBackoff> {
        // bail immediately if the host is in backoff
        if let Some(remaining) = self.backoff.backoff(SystemTime::now()) {
            return Err(InBackoff { remaining });
        }

        // get permit first, even though we might have to wait for the limiter
        // to actually start the request. helps prevent post-limit pile-ups,
        // gives visibility for backpressure
        let permit = self
            .request_concurrency
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore not closed");

        // slow down the actual request sending
        self.limiter.until_ready().await;

        // final check: in case we backed off while limit/permit waiting
        if let Some(remaining) = self.backoff.backoff(SystemTime::now()) {
            return Err(InBackoff { remaining });
        }

        Ok(permit)
    }

    pub fn at_max_concurrency(&self) -> bool {
        self.request_concurrency.available_permits() == 0
    }

    pub fn sync_compliance<S: StorageEngine>(
        &self,
        engine: &S,
    ) -> Result<Sync11State, crate::storage::LoadError<S::Error>> {
        if let Some(s) = self.sync_compliance.get() {
            return Ok(s);
        }
        let loaded = host_info::load(engine, self)?;
        Ok(self.sync_compliance.set_if_unset(loaded))
    }

    pub fn ratchet_strict<E: StorageError, B: StorageBatch<E>>(&self, batch: &mut B) {
        if self.sync_compliance.upgrade() {
            counter!(HOST_SYNC_COMPLIANCE_UPGRADES_TOTAL).increment(1);
            host_info::store_strict(batch, self)
        }
    }

    #[cfg(test)]
    pub(crate) fn raw(name: Hostname) -> Self {
        let hosts = HostRegistry::new_default();
        Self::new(
            name,
            hosts.client_handle(),
            DEFAULT_HOST_QPS,
            DEFAULT_HOST_CONCURRENCY,
            SyncScope::Everything,
        )
    }

    /// acquire and return all of this host's concurrency permits, so
    /// `at_max_concurrency()` reads true until the returned guards are dropped.
    #[cfg(test)]
    pub(crate) fn saturate_for_test(&self) -> Vec<tokio::sync::OwnedSemaphorePermit> {
        let mut permits = Vec::new();
        while let Ok(p) = self.request_concurrency.clone().try_acquire_owned() {
            permits.push(p);
        }
        permits
    }

    #[cfg(test)]
    pub(crate) fn compliance_is_loaded(&self) -> bool {
        self.sync_compliance.get().is_some()
    }
}

impl fmt::Debug for Host {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Host")
            .field("name", &self.name.as_str())
            .field("sync_compliance", &self.sync_compliance.get())
            .finish_non_exhaustive()
    }
}

// TODO: is the outer Arc actually enough to compare?
impl PartialEq for Host {
    fn eq(&self, h: &Host) -> bool {
        self.name.eq(&h.name)
    }
}

/// parse a Retry-After delay-seconds header
///
/// TODO: also support http-dates (use a date lib, don't hand-roll)
fn parse_retry_after(headers: &http::HeaderMap, now: SystemTime) -> Option<SystemTime> {
    let val = headers.get(http::header::RETRY_AFTER)?.to_str().ok()?;
    let secs = val
        .parse::<u64>()
        .inspect_err(|_| debug!(retry_after = %val, "unhandled Retry-After (http-date form)"))
        .ok()?;
    Some(now + Duration::from_secs(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_or_create_returns_same_arc_while_alive() {
        let reg = HostRegistry::new_default();
        let h1 = reg.get("h.com").unwrap();
        let h2 = reg.get("h.com").unwrap();
        assert!(Arc::ptr_eq(&h1, &h2), "live hosts must share Arc<Host>");
    }

    #[test]
    fn get_returns_fresh_arc_after_drop() {
        let reg = HostRegistry::new_default();
        let p1 = Arc::as_ptr(&reg.get("h.com").unwrap());
        // returned Arc dropped at end of statement; Weak now dead.
        let h2 = reg.get("h.com").unwrap();
        assert_ne!(p1, Arc::as_ptr(&h2), "dropped host should not be revived");
    }

    #[test]
    fn multiple_clones_share_the_same_host() {
        let reg = HostRegistry::new_default();
        let h1 = reg.get("h.com").unwrap();
        let h2 = Arc::clone(&h1);
        assert!(Arc::ptr_eq(&h1, &h2));
        drop(h1);
        // h2 still alive; registry still hands us the same one
        let h3 = reg.get("h.com").unwrap();
        assert!(Arc::ptr_eq(&h2, &h3));
    }

    #[test]
    fn prune_clears_dead_entries() {
        let reg = HostRegistry::new_default();
        reg.get("a.com").unwrap();
        reg.get("b.com").unwrap();
        reg.get("c.com").unwrap();
        // returns dropped at end-of-stmt; all 3 Weaks now dead.
        assert_eq!(reg.len(), 3, "dead entries still occupy the map");
        reg.prune();
        assert!(reg.is_empty(), "prune should remove dead Weaks");
    }

    #[test]
    fn hostname_inside_host_shares_arc_with_registry_key() {
        let reg = HostRegistry::new_default();
        let h = reg.get("h.com").unwrap();
        // the registry's map key wraps the same Arc<str> as h.name; both hold
        // strong refs to that one allocation.
        assert!(
            Arc::strong_count(h.name().arc()) >= 2,
            "name's Arc<str> should be shared between Host and registry map key",
        );
    }

    #[test]
    fn sync_compliance_starts_unset() {
        let reg = HostRegistry::new_default();
        let h = reg.get("h.com").unwrap();
        assert!(
            !h.compliance_is_loaded(),
            "ratchet must not load at intern time"
        );
    }

    #[test]
    fn hostname_equality_is_content_based() {
        let a = Hostname::new("h.com");
        let b = Hostname::new("h.com");
        assert_eq!(a, b);
        assert!(
            !Arc::ptr_eq(a.arc(), b.arc()),
            "two `new`s allocate independent Arcs"
        );
    }

    #[test]
    fn registry_normalizes_on_lookup() {
        let reg = HostRegistry::new_default();
        let h1 = reg.get("Example.COM").unwrap();
        let h2 = reg.get("example.com").unwrap();
        let h3 = reg.get("EXAMPLE.com:443").unwrap();
        let h4 = reg.get("example.com.").unwrap();
        assert!(Arc::ptr_eq(&h1, &h2));
        assert!(Arc::ptr_eq(&h1, &h3));
        assert!(Arc::ptr_eq(&h1, &h4));
        assert_eq!(h1.name().as_str(), "example.com");
    }

    #[test]
    fn registry_rejects_path_query_fragment() {
        let reg = HostRegistry::new_default();
        assert!(matches!(
            reg.get("example.com/path"),
            Err(HostnameError::NotBare(_))
        ));
        assert!(matches!(
            reg.get("example.com?q=1"),
            Err(HostnameError::NotBare(_))
        ));
        assert!(matches!(
            reg.get("example.com#frag"),
            Err(HostnameError::NotBare(_))
        ));
    }

    #[test]
    fn registry_rejects_userinfo() {
        let reg = HostRegistry::new_default();
        assert!(matches!(
            reg.get("user@example.com"),
            Err(HostnameError::NotBare(_))
        ));
        assert!(matches!(
            reg.get("user:pass@example.com"),
            Err(HostnameError::NotBare(_))
        ));
    }

    #[test]
    fn registry_rejects_scheme_prefixed() {
        let reg = HostRegistry::new_default();
        // wrapping "http://example.com" → "https://http://example.com" parses
        // with path=//example.com, caught by the NotBare check (or by the
        // Url parser itself, depending on the input). either way: rejected.
        assert!(reg.get("http://example.com").is_err());
        assert!(reg.get("https://example.com").is_err());
    }

    #[test]
    fn registry_rejects_empty_or_port_only() {
        let reg = HostRegistry::new_default();
        assert!(reg.get("").is_err());
        assert!(reg.get(":443").is_err());
        assert!(reg.get(".").is_err());
    }

    #[test]
    fn registry_normalizes_idn_to_punycode() {
        // locks in the `url` crate's IDN-to-punycode normalization: non-ASCII
        // labels in a hostname are converted to their xn-- punycode form.
        // changing this behavior later would silently re-key existing hosts,
        // so it's worth asserting.
        let reg = HostRegistry::new_default();
        let h = reg.get("bücher.example").unwrap();
        assert_eq!(h.name().as_str(), "xn--bcher-kva.example");
        // and the punycode form maps to the same Arc<Host>:
        let h2 = reg.get("xn--bcher-kva.example").unwrap();
        assert!(Arc::ptr_eq(&h, &h2));
    }

    #[test]
    fn compliance_cell_starts_unset() {
        let c = ComplianceCell::new_unset();
        assert_eq!(c.get(), None);
    }

    #[test]
    fn compliance_cell_set_if_unset_first_caller_wins() {
        let c = ComplianceCell::new_unset();
        assert_eq!(c.set_if_unset(Sync11State::Lax), Sync11State::Lax);
        assert_eq!(c.get(), Some(Sync11State::Lax));
        // a later call for the same cell returns the existing value, not the
        // newly-proposed one.
        assert_eq!(c.set_if_unset(Sync11State::Strict), Sync11State::Lax);
        assert_eq!(c.get(), Some(Sync11State::Lax));
    }

    #[test]
    fn compliance_cell_upgrade_from_unset() {
        let c = ComplianceCell::new_unset();
        assert!(c.upgrade(), "transition from unset should report true");
        assert_eq!(c.get(), Some(Sync11State::Strict));
    }

    #[test]
    fn compliance_cell_upgrade_from_lax() {
        let c = ComplianceCell::new_unset();
        c.set_if_unset(Sync11State::Lax);
        assert!(c.upgrade(), "transition from lax should report true");
        assert_eq!(c.get(), Some(Sync11State::Strict));
    }

    #[test]
    fn compliance_cell_upgrade_is_idempotent() {
        let c = ComplianceCell::new_unset();
        c.upgrade();
        assert!(!c.upgrade(), "subsequent upgrades report false (no-op)");
        assert_eq!(c.get(), Some(Sync11State::Strict));
    }

    // --- host-wide backoff ---

    /// millisecond-aligned synthetic time, so BackoffCell's ms-resolution
    /// storage round-trips exactly and asserts can use equality
    fn bt(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_000_000 + secs)
    }

    fn retry_after_headers(val: &str) -> http::HeaderMap {
        let mut h = http::HeaderMap::new();
        h.insert(
            http::header::RETRY_AFTER,
            val.parse().expect("header value"),
        );
        h
    }

    #[test]
    fn backoff_cell_starts_expired() {
        let c = BackoffCell::new();
        assert_eq!(c.backoff(bt(0)), None);
    }

    #[test]
    fn backoff_cell_push_to_sets_remaining() {
        let c = BackoffCell::new();
        c.push_to(bt(10), bt(0));
        assert_eq!(c.backoff(bt(0)), Some(Duration::from_secs(10)));
        assert_eq!(
            c.backoff(bt(11)),
            None,
            "a passed deadline reads as no backoff"
        );
    }

    #[test]
    fn backoff_cell_only_advances() {
        let c = BackoffCell::new();
        c.push_to(bt(60), bt(0));
        c.push_to(bt(10), bt(0));
        assert_eq!(
            c.backoff(bt(0)),
            Some(Duration::from_secs(60)),
            "a shorter push must not pull the deadline back",
        );
    }

    #[test]
    fn backoff_cell_caps_at_max() {
        let c = BackoffCell::new();
        c.push_to(bt(100_000), bt(0));
        assert_eq!(c.backoff(bt(0)), Some(DEFAULT_MAX_BACKOFF));
    }

    #[test]
    fn observe_response_429_honors_retry_after() {
        let host = Host::raw(Hostname::new("h.com"));
        let before = SystemTime::now();
        host.observe_response(
            http::StatusCode::TOO_MANY_REQUESTS,
            &retry_after_headers("5"),
        );
        // the deadline is ~5s after observe_response's own `now`, which is at
        // or slightly after `before`: probe either side with margin
        assert!(
            host.backoff_remaining(before + Duration::from_secs(4))
                .is_some()
        );
        assert!(
            host.backoff_remaining(before + Duration::from_secs(7))
                .is_none()
        );
    }

    #[test]
    fn observe_response_429_default_without_retry_after() {
        let host = Host::raw(Hostname::new("h.com"));
        let before = SystemTime::now();
        host.observe_response(http::StatusCode::TOO_MANY_REQUESTS, &http::HeaderMap::new());
        assert!(
            host.backoff_remaining(before + Duration::from_secs(59))
                .is_some()
        );
        assert!(
            host.backoff_remaining(before + Duration::from_secs(62))
                .is_none()
        );
    }

    #[test]
    fn observe_response_transient_5xx_backs_off() {
        use http::StatusCode as S;
        for status in [S::BAD_GATEWAY, S::SERVICE_UNAVAILABLE, S::GATEWAY_TIMEOUT] {
            let host = Host::raw(Hostname::new("h.com"));
            let before = SystemTime::now();
            host.observe_response(status, &http::HeaderMap::new());
            assert!(
                host.backoff_remaining(before + Duration::from_secs(29))
                    .is_some(),
                "{status} should back off",
            );
            assert!(
                host.backoff_remaining(before + Duration::from_secs(32))
                    .is_none(),
                "{status} should use the shorter transient default",
            );
        }
    }

    #[test]
    fn observe_response_ignores_non_backoff_statuses() {
        use http::StatusCode as S;
        let host = Host::raw(Hostname::new("h.com"));
        // retry-after included to prove ignoring is status-based, not
        // header-based. plain 500 is deliberately not a backoff trigger.
        for status in [
            S::OK,
            S::NOT_FOUND,
            S::BAD_REQUEST,
            S::INTERNAL_SERVER_ERROR,
        ] {
            host.observe_response(status, &retry_after_headers("60"));
        }
        assert_eq!(host.backoff_remaining(SystemTime::now()), None);
    }

    #[test]
    fn observe_response_caps_huge_retry_after() {
        let host = Host::raw(Hostname::new("h.com"));
        let before = SystemTime::now();
        host.observe_response(
            http::StatusCode::TOO_MANY_REQUESTS,
            &retry_after_headers("100000"),
        );
        let remaining = host.backoff_remaining(before).expect("backing off");
        assert!(
            remaining <= DEFAULT_MAX_BACKOFF + Duration::from_secs(2),
            "hostile Retry-After must be capped, got {remaining:?}",
        );
    }

    #[test]
    fn observe_response_only_extends_backoff() {
        let host = Host::raw(Hostname::new("h.com"));
        let before = SystemTime::now();
        host.observe_response(
            http::StatusCode::TOO_MANY_REQUESTS,
            &retry_after_headers("300"),
        );
        // a later, shorter signal (transient default) must not shorten it
        host.observe_response(
            http::StatusCode::SERVICE_UNAVAILABLE,
            &http::HeaderMap::new(),
        );
        assert!(
            host.backoff_remaining(before + Duration::from_secs(299))
                .is_some()
        );
    }

    #[tokio::test]
    async fn request_permit_immediate_without_backoff() {
        let host = Host::raw(Hostname::new("h.com"));
        let _permit = host.request_permit().await.expect("not backing off"); // must not hang
    }

    #[tokio::test]
    async fn request_permit_fails_fast_during_backoff() {
        let host = Host::raw(Hostname::new("h.com"));
        // 60s default backoff: the test only finishes quickly if the permit
        // refuses instead of sleeping the backoff out
        host.observe_response(http::StatusCode::TOO_MANY_REQUESTS, &http::HeaderMap::new());

        let InBackoff { remaining } = host
            .request_permit()
            .await
            .expect_err("must refuse while backing off");
        assert!(
            remaining <= DEFAULT_RATELIMIT_BACKOFF && remaining > Duration::from_secs(58),
            "remaining should reflect the backoff deadline, got {remaining:?}",
        );
    }

    #[tokio::test]
    async fn request_permit_allowed_again_after_backoff_expires() {
        let host = Host::raw(Hostname::new("h.com"));
        let now = SystemTime::now();
        host.backoff.push_to(now + Duration::from_millis(100), now);
        host.request_permit().await.expect_err("still backing off");

        tokio::time::sleep(Duration::from_millis(150)).await;
        let _permit = host
            .request_permit()
            .await
            .expect("backoff expired; permits flow again");
    }

    async fn wait_for(label: &str, mut pred: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !pred() {
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for: {label}",
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// drain the limiter's burst allowance without keeping any slots, so the
    /// next caller must wait on rate
    async fn burn_rate_burst(host: &Host) {
        for _ in 0..DEFAULT_HOST_QPS.get() {
            drop(host.request_permit().await.expect("burst permit"));
        }
    }

    #[tokio::test]
    async fn request_permit_holds_slot_while_rate_limited() {
        // the in-flight slot is taken *before* the rate-limiter wait, so
        // callers parked on rate show up in at_max_concurrency() -- the
        // scheduler's dispatch gate. (regression: waiters used to line up
        // invisibly at the limiter, letting the scheduler dispatch a whole
        // host backlog into the queue.)
        let host = Arc::new(Host::raw(Hostname::new("h.com")));
        burn_rate_burst(&host).await;

        // fill every slot with callers that can only be waiting on rate
        let waiters: Vec<_> = (0..DEFAULT_HOST_CONCURRENCY)
            .map(|_| {
                let h = host.clone();
                tokio::spawn(async move { h.request_permit().await })
            })
            .collect();

        wait_for("all slots taken by rate-waiters", || {
            host.at_max_concurrency()
        })
        .await;
        assert!(
            waiters.iter().all(|w| !w.is_finished()),
            "waiters should still be parked at the limiter, holding their slots",
        );

        // they eventually pass the limiter and get real permits
        for w in waiters {
            let _permit = w.await.expect("no panic").expect("not backing off");
        }
        assert!(!host.at_max_concurrency(), "all slots released");
    }

    #[tokio::test]
    async fn request_permit_rechecks_backoff_after_rate_wait() {
        // a backoff arming *while* a caller waits in line must refuse it on
        // wake instead of letting it drain onto a still-429ing host -- and
        // must release its slot on the way out
        let host = Arc::new(Host::raw(Hostname::new("h.com")));
        burn_rate_burst(&host).await;

        let h = host.clone();
        let waiter = tokio::spawn(async move { h.request_permit().await });

        // it holds a slot: past the entry backoff check, parked on rate
        wait_for("waiter holding a slot", || {
            host.request_concurrency.available_permits() < DEFAULT_HOST_CONCURRENCY
        })
        .await;

        // arm the backoff behind its back
        let now = SystemTime::now();
        host.backoff.push_to(now + Duration::from_secs(60), now);

        let res = waiter.await.expect("no panic");
        assert!(res.is_err(), "woken waiter must be refused, not sent");
        assert_eq!(
            host.request_concurrency.available_permits(),
            DEFAULT_HOST_CONCURRENCY,
            "slot released on the backoff-refusal path",
        );
    }
}
