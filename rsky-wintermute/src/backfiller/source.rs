//! Where backfill gets its repos: enumeration, and archive fetch.
//!
//! The current backfiller hard-wires one answer -- enumerate DIDs from a relay,
//! resolve each one to a PDS, fetch from there. This splits that into a trait so
//! hubble and PDS-direct are interchangeable, and so the retry and give-up
//! policy lives in one tested place instead of inline in the worker loop
//! (`retry_count < 2`, then dead-letter, with no notion of *why* it failed).

use std::time::Duration;

/// One repo, as an enumeration source describes it.
///
/// `rev` is the whole reason this type exists. It is what lets a later
/// enumeration pass skip a repo whose contents have not moved. The current
/// producer parses `listRepos` into `{ did }` and throws the rest away, which
/// is why every pass re-enqueues the entire network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRef {
    pub did: String,
    pub rev: String,
    pub active: bool,
    pub status: Option<String>,
}

/// One page of enumeration results.
///
/// `cursor` is a `String`, not an `i64`. Relay cursors are integers; hubble's
/// are DIDs. The existing producer persists with `parse::<i64>().unwrap_or(0)`,
/// so against any non-numeric cursor it writes `0` and every restart
/// re-enumerates from the beginning of the keyspace.
#[derive(Debug, Clone, Default)]
pub struct RepoPage {
    pub repos: Vec<RepoRef>,
    pub cursor: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    /// A response we got and understood, but which was not a success.
    #[error("http {status}")]
    Status {
        status: u16,
        retry_after_secs: Option<u64>,
    },
    /// No HTTP response at all: DNS, connect, TLS, timeout, read.
    #[error("transport: {0}")]
    Transport(String),
    /// A 2xx whose body we could not make sense of.
    #[error("decode: {0}")]
    Decode(String),
    /// Body exceeded the configured cap before we finished reading it.
    #[error("body too large: {got} bytes exceeds cap {cap}")]
    TooLarge { got: u64, cap: u64 },
}

/// What kind of failure this was, which decides whether we back off and retry,
/// throttle the host, or stop asking.
///
/// Ported from `car-dump`'s `Classification`. The distinction that matters:
/// a 429 or a 502 means "later"; a 400 or a 404 means "never", and retrying it
/// only burns budget that a recoverable host could have used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Rate limited. Back off, and respect `Retry-After` if it came with one.
    RateLimited,
    /// Host-side 5xx. Transient, throttle the host.
    Server5xx,
    /// No response. Transient, throttle the host.
    Transport,
    /// The host will never serve this. Do not retry, do not throttle the host
    /// for it -- it is this repo that is unserveable, not the host.
    Terminal,
}

impl Class {
    /// Whether this is worth trying again later.
    #[must_use]
    pub const fn is_transient(self) -> bool {
        matches!(self, Self::RateLimited | Self::Server5xx | Self::Transport)
    }

    /// Whether this outcome should count against the host's health, stepping
    /// its concurrency down. Terminal per-repo failures must not: a host that
    /// legitimately 404s a thousand deleted repos is healthy.
    #[must_use]
    pub const fn throttles_host(self) -> bool {
        self.is_transient()
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RateLimited => "rate_limited",
            Self::Server5xx => "server_5xx",
            Self::Transport => "transport",
            Self::Terminal => "terminal",
        }
    }
}

/// Classify a failure.
///
/// 501 and 505 are terminal even though they are 5xx: the host is telling us it
/// will never implement this, so a retry budget spent on it is wasted.
#[must_use]
pub fn classify(err: &SourceError) -> Class {
    match err {
        SourceError::Transport(_) => Class::Transport,
        SourceError::Status { status, .. } => match *status {
            429 => Class::RateLimited,
            501 | 505 => Class::Terminal,
            s if (500..600).contains(&s) => Class::Server5xx,
            _ => Class::Terminal,
        },
        SourceError::Decode(_) | SourceError::TooLarge { .. } => Class::Terminal,
    }
}

/// Server-supplied wait hint, if the failure carried one.
#[must_use]
pub const fn retry_after_secs(err: &SourceError) -> Option<u64> {
    match err {
        SourceError::Status {
            retry_after_secs, ..
        } => *retry_after_secs,
        _ => None,
    }
}

/// Base for capped exponential backoff. `attempt` is 1-indexed.
pub const BACKOFF_BASE_SECS: u64 = 1;
/// Ceiling on a single backoff sleep.
pub const BACKOFF_MAX_SECS: u64 = 60;
/// Floor on a host cooldown, used when the server sent no `Retry-After`.
pub const COOLDOWN_FLOOR_SECS: u64 = 10;
/// Ceiling on a host cooldown, so a hostile `Retry-After: 99999999` cannot pin
/// a host out of rotation forever.
pub const COOLDOWN_MAX_SECS: u64 = 300;

/// Capped exponential backoff: 1, 2, 4, 8, ... clamped to [`BACKOFF_MAX_SECS`].
#[must_use]
pub const fn backoff_secs(attempt: u32) -> u64 {
    let exp = attempt.saturating_sub(1);
    let exp = if exp > 6 { 6 } else { exp };
    let v = BACKOFF_BASE_SECS << exp;
    if v > BACKOFF_MAX_SECS {
        BACKOFF_MAX_SECS
    } else {
        v
    }
}

/// How long to park a host after a rate limit, honouring the server's hint
/// when it gave one but never trusting it past [`COOLDOWN_MAX_SECS`].
#[must_use]
pub const fn cooldown_secs(retry_after: Option<u64>) -> u64 {
    match retry_after {
        Some(secs) if secs > COOLDOWN_FLOOR_SECS => {
            if secs > COOLDOWN_MAX_SECS {
                COOLDOWN_MAX_SECS
            } else {
                secs
            }
        }
        _ => COOLDOWN_FLOOR_SECS,
    }
}

/// Parse `Retry-After` in its integer-seconds form.
///
/// The HTTP-date form is deliberately not handled: it is rare in practice, and
/// a miss yields `None`, which falls back to the floor -- the safe direction.
#[must_use]
pub fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
}

/// An archive body. Small repos stay in memory; the fat tail spills to disk.
///
/// The measured distribution justifies the split: p50 is 3 KiB and p90 is
/// 135 KiB, but p99.9 is 24 MiB and the largest repo sampled was 87 MB. Holding
/// the tail in memory across N workers is what makes the current backfiller's
/// footprint unpredictable -- and it holds each one *three* times over
/// (`Bytes`, then `to_vec()`, then the `BlockMap`).
#[derive(Debug)]
pub enum RepoBody {
    Memory(Vec<u8>),
    Spilled {
        file: tempfile::NamedTempFile,
        len: u64,
    },
}

impl RepoBody {
    #[must_use]
    pub fn len(&self) -> u64 {
        match self {
            Self::Memory(v) => v.len() as u64,
            Self::Spilled { len, .. } => *len,
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[must_use]
    pub const fn spilled(&self) -> bool {
        matches!(self, Self::Spilled { .. })
    }
}

/// A place backfill can enumerate repos from and fetch archives from.
///
/// Static dispatch: the runner is generic over the source. Runtime switching
/// between hubble and PDS-direct wants an enum wrapper rather than `dyn`,
/// because the futures here are opaque.
pub trait RepoSource: Send + Sync {
    /// Short name for logs and metric labels.
    fn name(&self) -> &'static str;

    /// One page of enumeration. `cursor` is opaque and source-defined.
    fn list_repos(
        &self,
        cursor: Option<&str>,
        limit: u32,
    ) -> impl Future<Output = Result<RepoPage, SourceError>> + Send;

    /// Fetch one repo's CAR, spilling to disk past `spill_threshold` and
    /// refusing past `cap`.
    fn fetch_repo(
        &self,
        did: &str,
        spill_threshold: u64,
        cap: u64,
    ) -> impl Future<Output = Result<RepoBody, SourceError>> + Send;
}

/// Sleep helper so the runner's backoff is easy to see at the call site.
pub async fn sleep_secs(secs: u64) {
    tokio::time::sleep(Duration::from_secs(secs)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(code: u16) -> SourceError {
        SourceError::Status {
            status: code,
            retry_after_secs: None,
        }
    }

    #[test]
    fn rate_limits_and_server_errors_are_transient() {
        assert_eq!(classify(&status(429)), Class::RateLimited);
        for code in [500, 502, 503, 504] {
            assert_eq!(classify(&status(code)), Class::Server5xx, "{code}");
            assert!(classify(&status(code)).is_transient());
        }
        assert!(classify(&SourceError::Transport("dns".into())).is_transient());
    }

    #[test]
    fn client_errors_and_unimplemented_are_terminal() {
        for code in [400, 401, 403, 404, 410, 501, 505] {
            assert_eq!(classify(&status(code)), Class::Terminal, "{code}");
            assert!(!classify(&status(code)).is_transient());
        }
        assert_eq!(
            classify(&SourceError::Decode("bad cbor".into())),
            Class::Terminal
        );
        assert_eq!(
            classify(&SourceError::TooLarge { got: 9, cap: 8 }),
            Class::Terminal
        );
    }

    #[test]
    fn terminal_failures_do_not_throttle_the_host() {
        // A host serving a thousand 404s for deleted repos is healthy; only
        // transient failures are evidence about the host itself.
        assert!(!classify(&status(404)).throttles_host());
        assert!(classify(&status(429)).throttles_host());
        assert!(classify(&status(503)).throttles_host());
    }

    #[test]
    fn backoff_grows_then_caps() {
        assert_eq!(backoff_secs(1), 1);
        assert_eq!(backoff_secs(2), 2);
        assert_eq!(backoff_secs(3), 4);
        assert_eq!(backoff_secs(4), 8);
        assert_eq!(backoff_secs(7), BACKOFF_MAX_SECS);
        assert_eq!(backoff_secs(u32::MAX), BACKOFF_MAX_SECS);
        assert_eq!(backoff_secs(0), 1, "0 must not underflow");
    }

    #[test]
    fn cooldown_respects_the_server_but_not_blindly() {
        assert_eq!(cooldown_secs(None), COOLDOWN_FLOOR_SECS);
        assert_eq!(cooldown_secs(Some(1)), COOLDOWN_FLOOR_SECS, "below floor");
        assert_eq!(cooldown_secs(Some(45)), 45, "server hint honoured");
        assert_eq!(
            cooldown_secs(Some(99_999_999)),
            COOLDOWN_MAX_SECS,
            "hostile hint clamped"
        );
    }

    #[test]
    fn retry_after_parses_integer_seconds_only() {
        let mut h = reqwest::header::HeaderMap::new();
        assert_eq!(parse_retry_after(&h), None);
        h.insert(reqwest::header::RETRY_AFTER, " 30 ".parse().unwrap());
        assert_eq!(parse_retry_after(&h), Some(30));
        h.insert(
            reqwest::header::RETRY_AFTER,
            "Wed, 21 Oct 2026 07:28:00 GMT".parse().unwrap(),
        );
        assert_eq!(parse_retry_after(&h), None, "http-date falls back to floor");
    }

    #[test]
    fn retry_after_is_carried_through_classification() {
        let err = SourceError::Status {
            status: 429,
            retry_after_secs: Some(20),
        };
        assert_eq!(classify(&err), Class::RateLimited);
        assert_eq!(retry_after_secs(&err), Some(20));
        assert_eq!(cooldown_secs(retry_after_secs(&err)), 20);
    }

    #[test]
    fn body_reports_its_own_size_and_placement() {
        let mem = RepoBody::Memory(vec![0u8; 1024]);
        assert_eq!(mem.len(), 1024);
        assert!(!mem.spilled());
        assert!(!mem.is_empty());
        assert!(RepoBody::Memory(Vec::new()).is_empty());
    }
}
