//! Where backfill gets its repos: enumeration, and archive fetch.
//!
//! Two sources implement [`RepoSource`]: hubble, the public whole-network
//! mirror, and a PDS reached directly. The runner is generic over both, and the
//! retry and give-up policy lives here in one tested place instead of inline in
//! a worker loop.

use std::path::Path;
use std::time::Duration;

use tokio::io::AsyncWriteExt;

/// One repo, as an enumeration source describes it.
///
/// `rev` is what lets a later enumeration pass skip a repo whose contents have
/// not moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRef {
    pub did: String,
    pub rev: String,
    pub active: bool,
    pub status: Option<String>,
}

/// One page of enumeration results. `cursor` is opaque text: relay cursors are
/// integers, hubble's are DIDs.
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
        /// The body was an XRPC error envelope (`{"error": ..., "message": ...}`).
        /// A 500 carrying one is the PDS reporting a per-repo failure, not host
        /// load, and must not throttle the host.
        xrpc_error: bool,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Rate limited. Back off, and respect `Retry-After` if it came with one.
    RateLimited,
    /// Host-side or proxy 5xx. Transient, throttles the host.
    Server5xx,
    /// A 500 with an XRPC error body: this repo, not this host. Transient, but
    /// says nothing about host health.
    Server5xxApp,
    /// No response. Transient, throttles the host.
    Transport,
    /// The host will never serve this. Do not retry, do not throttle the host
    /// for it.
    Terminal,
}

impl Class {
    /// Whether this is worth trying again later.
    #[must_use]
    pub const fn is_transient(self) -> bool {
        !matches!(self, Self::Terminal)
    }

    /// Whether this outcome is evidence about the host, stepping its
    /// concurrency down. A host that 404s a thousand deleted repos is healthy.
    #[must_use]
    pub const fn throttles_host(self) -> bool {
        matches!(self, Self::RateLimited | Self::Server5xx | Self::Transport)
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RateLimited => "rate_limited",
            Self::Server5xx => "server_5xx",
            Self::Server5xxApp => "server_5xx_app",
            Self::Transport => "transport",
            Self::Terminal => "terminal",
        }
    }
}

/// Classify a failure. 501 and 505 are terminal even though they are 5xx: the
/// host is telling us it will never implement this.
#[must_use]
pub fn classify(err: &SourceError) -> Class {
    match err {
        SourceError::Transport(_) => Class::Transport,
        SourceError::Status {
            status, xrpc_error, ..
        } => match *status {
            429 => Class::RateLimited,
            501 | 505 => Class::Terminal,
            500 if *xrpc_error => Class::Server5xxApp,
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

/// Parse `Retry-After` in its integer-seconds form. The HTTP-date form is not
/// handled; a miss yields `None`, which falls back to the floor.
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

/// True if `body` is the XRPC error envelope: an object with string `error`
/// and `message` fields.
#[must_use]
pub fn is_xrpc_error_body(body: &[u8]) -> bool {
    let Ok(serde_json::Value::Object(map)) = serde_json::from_slice::<serde_json::Value>(body)
    else {
        return false;
    };
    map.get("error").is_some_and(serde_json::Value::is_string)
        && map.get("message").is_some_and(serde_json::Value::is_string)
}

/// Largest error body we bother reading, to classify it.
const MAX_ERROR_BODY: usize = 2048;

/// Turn a non-2xx response into a [`SourceError::Status`], reading a bounded
/// slice of the body to tell an XRPC application error from a proxy 5xx.
pub async fn status_error(resp: reqwest::Response) -> SourceError {
    let status = resp.status().as_u16();
    let retry_after_secs = parse_retry_after(resp.headers());
    let mut body: Vec<u8> = Vec::new();
    let mut resp = resp;
    while let Ok(Some(chunk)) = resp.chunk().await {
        body.extend_from_slice(&chunk);
        if body.len() >= MAX_ERROR_BODY {
            break;
        }
    }
    SourceError::Status {
        status,
        retry_after_secs,
        xrpc_error: is_xrpc_error_body(&body),
    }
}

/// An archive body. Small repos stay in memory; the fat tail spills to disk.
///
/// Measured: p50 is 3 KiB and p90 is 135 KiB, but p99.9 is 24 MiB and the
/// largest repo sampled was 87 MB. Holding the tail in memory across N workers
/// is what made the old backfiller's footprint unpredictable.
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
    pub const fn len(&self) -> u64 {
        match self {
            Self::Memory(v) => v.len() as u64,
            Self::Spilled { len, .. } => *len,
        }
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[must_use]
    pub const fn spilled(&self) -> bool {
        matches!(self, Self::Spilled { .. })
    }
}

/// Stream a response body into a [`RepoBody`].
///
/// Spills to a file in `spill_dir` once `spill_threshold` bytes have arrived
/// and refuses past `cap`. Shared by every source so the memory bound is
/// enforced in one place.
pub async fn read_body(
    mut resp: reqwest::Response,
    spill_threshold: u64,
    cap: u64,
    spill_dir: &Path,
) -> Result<RepoBody, SourceError> {
    let mut buf: Vec<u8> = Vec::new();
    let mut spill: Option<(tempfile::NamedTempFile, tokio::fs::File)> = None;
    let mut total: u64 = 0;

    loop {
        let chunk = resp
            .chunk()
            .await
            .map_err(|e| SourceError::Transport(format!("body: {e}")))?;
        let Some(chunk) = chunk else { break };

        total += chunk.len() as u64;
        if total > cap {
            return Err(SourceError::TooLarge { got: total, cap });
        }

        if let Some((_, file)) = spill.as_mut() {
            file.write_all(&chunk)
                .await
                .map_err(|e| SourceError::Transport(format!("spill write: {e}")))?;
        } else if total > spill_threshold {
            let tmp = tempfile::NamedTempFile::new_in(spill_dir)
                .map_err(|e| SourceError::Transport(format!("spill create: {e}")))?;
            let handle = tmp
                .reopen()
                .map_err(|e| SourceError::Transport(format!("spill reopen: {e}")))?;
            let mut file = tokio::fs::File::from_std(handle);
            file.write_all(&buf)
                .await
                .map_err(|e| SourceError::Transport(format!("spill write: {e}")))?;
            file.write_all(&chunk)
                .await
                .map_err(|e| SourceError::Transport(format!("spill write: {e}")))?;
            buf = Vec::new();
            spill = Some((tmp, file));
        } else {
            buf.extend_from_slice(&chunk);
        }
    }

    if let Some((tmp, mut file)) = spill {
        file.flush()
            .await
            .map_err(|e| SourceError::Transport(format!("spill flush: {e}")))?;
        drop(file);
        return Ok(RepoBody::Spilled {
            file: tmp,
            len: total,
        });
    }
    Ok(RepoBody::Memory(buf))
}

/// Fetch tunables shared by every source.
#[derive(Debug, Clone)]
pub struct FetchLimits {
    /// Bodies larger than this spill to a file instead of staying in memory.
    pub spill_threshold: u64,
    /// Refuse a body larger than this outright.
    pub max_body: u64,
    /// Where spilled bodies go.
    pub spill_dir: std::path::PathBuf,
    /// Whole-request timeout for one archive.
    pub request_timeout: Duration,
}

impl Default for FetchLimits {
    fn default() -> Self {
        Self {
            // p90 is 135 KiB, so this keeps ~9 repos in 10 off the disk.
            spill_threshold: 1 << 20,
            // The largest repo sampled was 87 MB; 512 MiB guards against a
            // pathological response, it is not a working limit.
            max_body: 512 << 20,
            spill_dir: std::env::temp_dir(),
            request_timeout: Duration::from_secs(300),
        }
    }
}

/// A place backfill can enumerate repos from and fetch archives from.
///
/// One attempt per call: retry policy belongs to the runner, which knows
/// whether it is enumerating (never lose a page) or fetching (the repo can be
/// revisited from persisted state).
pub trait RepoSource: Send + Sync {
    /// Identifier for state rows, logs and metric labels: `hubble`, or the
    /// PDS hostname.
    fn name(&self) -> &str;

    /// One page of enumeration. `cursor` is opaque and source-defined.
    fn list_repos(
        &self,
        cursor: Option<String>,
        limit: u32,
    ) -> impl Future<Output = Result<RepoPage, SourceError>> + Send;

    /// Fetch one repo's CAR.
    fn fetch_repo(
        &self,
        did: String,
        limits: FetchLimits,
    ) -> impl Future<Output = Result<RepoBody, SourceError>> + Send;
}

/// Whether a `listRepos` status means the archive will never be fetchable, so
/// the row goes straight to terminal instead of spending a `getRepo` that
/// will 400.
#[must_use]
pub fn status_is_unfetchable(active: bool, status: Option<&str>) -> bool {
    if active {
        return false;
    }
    matches!(
        status,
        Some("deleted" | "takendown" | "deactivated" | "suspended") | None
    )
}

/// Sleep helper so the runner's backoff is easy to see at the call site.
pub async fn sleep_secs(secs: u64) {
    tokio::time::sleep(Duration::from_secs(secs)).await;
}

#[cfg(test)]
mod tests {
    #![allow(clippy::significant_drop_tightening)]
    use super::*;

    fn status(code: u16) -> SourceError {
        SourceError::Status {
            status: code,
            retry_after_secs: None,
            xrpc_error: false,
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
    fn a_500_with_an_xrpc_body_is_a_repo_problem_not_a_host_problem() {
        let err = SourceError::Status {
            status: 500,
            retry_after_secs: None,
            xrpc_error: true,
        };
        assert_eq!(classify(&err), Class::Server5xxApp);
        assert!(classify(&err).is_transient());
        assert!(!classify(&err).throttles_host());
        // Only 500 gets that reading: a 502 with a JSON body is still a proxy.
        let proxy = SourceError::Status {
            status: 502,
            retry_after_secs: None,
            xrpc_error: true,
        };
        assert_eq!(classify(&proxy), Class::Server5xx);
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
        assert!(!classify(&status(404)).throttles_host());
        assert!(classify(&status(429)).throttles_host());
        assert!(classify(&status(503)).throttles_host());
        assert!(classify(&SourceError::Transport("x".into())).throttles_host());
        assert_eq!(Class::Server5xxApp.as_str(), "server_5xx_app");
        assert_eq!(Class::Terminal.as_str(), "terminal");
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
            xrpc_error: false,
        };
        assert_eq!(classify(&err), Class::RateLimited);
        assert_eq!(retry_after_secs(&err), Some(20));
        assert_eq!(retry_after_secs(&SourceError::Transport("x".into())), None);
        assert_eq!(cooldown_secs(retry_after_secs(&err)), 20);
    }

    #[test]
    fn xrpc_error_bodies_are_recognised() {
        assert!(is_xrpc_error_body(
            br#"{"error":"RepoNotFound","message":"Could not find repo"}"#
        ));
        assert!(!is_xrpc_error_body(br#"{"error":"x"}"#));
        assert!(!is_xrpc_error_body(b"<html>502 bad gateway</html>"));
        assert!(!is_xrpc_error_body(b""));
    }

    #[test]
    fn inactive_repos_are_recognised_as_unfetchable() {
        assert!(!status_is_unfetchable(true, None));
        assert!(!status_is_unfetchable(true, Some("takendown")));
        assert!(status_is_unfetchable(false, Some("deleted")));
        assert!(status_is_unfetchable(false, Some("takendown")));
        assert!(status_is_unfetchable(false, Some("deactivated")));
        assert!(status_is_unfetchable(false, Some("suspended")));
        assert!(status_is_unfetchable(false, None));
        assert!(!status_is_unfetchable(false, Some("throttled")));
        assert!(!status_is_unfetchable(false, Some("desynchronized")));
    }

    #[test]
    fn body_reports_its_own_size_and_placement() {
        let mem = RepoBody::Memory(vec![0u8; 1024]);
        assert_eq!(mem.len(), 1024);
        assert!(!mem.spilled());
        assert!(!mem.is_empty());
        assert!(RepoBody::Memory(Vec::new()).is_empty());
        let limits = FetchLimits::default();
        assert!(limits.spill_threshold < limits.max_body);
    }

    #[tokio::test]
    async fn bodies_spill_past_the_threshold_and_refuse_past_the_cap() {
        let mut server = mockito::Server::new_async().await;
        let big = vec![7u8; 4096];
        let _m = server
            .mock("GET", "/big")
            .with_status(200)
            .with_body(big.clone())
            .create_async()
            .await;
        let client = reqwest::Client::new();
        let dir = tempfile::tempdir().unwrap();

        let resp = client.get(server.url() + "/big").send().await.unwrap();
        let body = read_body(resp, 1024, 1 << 20, dir.path()).await.unwrap();
        assert!(body.spilled());
        assert_eq!(body.len(), 4096);
        if let RepoBody::Spilled { file, .. } = &body {
            assert_eq!(std::fs::read(file.path()).unwrap(), big);
        }

        let resp = client.get(server.url() + "/big").send().await.unwrap();
        let body = read_body(resp, 1 << 20, 1 << 20, dir.path()).await.unwrap();
        assert!(!body.spilled());
        assert_eq!(body.len(), 4096);

        let resp = client.get(server.url() + "/big").send().await.unwrap();
        let err = read_body(resp, 1024, 2048, dir.path()).await.unwrap_err();
        assert!(matches!(err, SourceError::TooLarge { cap: 2048, .. }));
    }

    #[tokio::test]
    async fn status_errors_carry_retry_after_and_xrpc_shape() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/limited")
            .with_status(429)
            .with_header("retry-after", "17")
            .with_body("slow down")
            .create_async()
            .await;
        let _m2 = server
            .mock("GET", "/broken")
            .with_status(500)
            .with_body(r#"{"error":"InternalServerError","message":"repo is broken"}"#)
            .create_async()
            .await;
        let client = reqwest::Client::new();

        let resp = client.get(server.url() + "/limited").send().await.unwrap();
        let err = status_error(resp).await;
        assert!(matches!(
            err,
            SourceError::Status {
                status: 429,
                retry_after_secs: Some(17),
                xrpc_error: false
            }
        ));

        let resp = client.get(server.url() + "/broken").send().await.unwrap();
        let err = status_error(resp).await;
        assert_eq!(classify(&err), Class::Server5xxApp);
    }
}
