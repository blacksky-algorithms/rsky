//! [`RepoSource`] backed by hubble, the public whole-network mirror.
//!
//! Why this is the default source rather than each repo's own PDS:
//!
//! * One host instead of thousands, so there is one rate budget to respect
//!   rather than a per-host health model to maintain.
//! * No DID resolution. hubble takes the DID directly, which deletes the
//!   per-job `IdResolver::new()` (fresh HTTP client, fresh DNS resolver, no
//!   timeout) that the current backfiller constructs once per repo.
//! * Measured from nyc3: 9 repos/s at concurrency 1 rising linearly to 119 at
//!   concurrency 16, with zero 429s across 15,311 requests. The knee is a
//!   ~28 MB/s byte ceiling, not a request limit.
//!
//! That last number is why the defaults here are deliberately far below what
//! the service will give us. wintermute writes ~521 records/s; hubble at the
//! knee supplies ~72,000. Pulling anywhere near capacity just moves the
//! bottleneck into a queue, which is the bug this replaces.

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};
use serde::Deserialize;
use tokio::io::AsyncWriteExt;

use super::source::{
    RepoBody, RepoPage, RepoRef, RepoSource, SourceError, backoff_secs, classify,
    parse_retry_after, sleep_secs,
};

/// Public instance. The docs host (`web.hubble.microcosm.blue`) is a different
/// service and does not serve XRPC.
pub const DEFAULT_BASE_URL: &str = "https://hubble.microcosm.blue";

/// Identifies this app to hubble, with a contact address.
///
/// hubble's policy requires one. Every `reqwest` client in wintermute currently
/// sends no user-agent at all, which is a violation, not just impoliteness.
pub const DEFAULT_USER_AGENT: &str =
    "rsky-wintermute/0.9 (+https://blacksky.app; clinton@blacksky.app)";

/// `listRepos` caps at 1000.
pub const MAX_PAGE_LIMIT: u32 = 1000;

/// Default self-imposed request rate, against a service measured serving 119/s
/// without complaint. Written as a `match` because `unwrap` is not permitted
/// here and this needs to be a `const`.
pub const DEFAULT_RPS: NonZeroU32 = match NonZeroU32::new(8) {
    Some(v) => v,
    None => NonZeroU32::MIN,
};

/// Retries for one enumeration page. Losing a page silently skips repos, so
/// this is more generous than a per-repo fetch, which the caller can revisit
/// from persisted state.
const LIST_MAX_RETRIES: u32 = 10;

#[derive(Debug, Deserialize)]
struct ListReposBody {
    repos: Vec<ListReposRepo>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListReposRepo {
    did: String,
    #[serde(default)]
    rev: String,
    #[serde(default)]
    active: Option<bool>,
    #[serde(default)]
    status: Option<String>,
}

/// Tunables, all well below measured capacity by intent.
#[derive(Debug, Clone)]
pub struct HubbleConfig {
    pub base_url: String,
    pub user_agent: String,
    /// Requests per second, enforced by a token bucket.
    pub rps: NonZeroU32,
    /// Bodies larger than this spill to a file instead of staying in memory.
    pub spill_threshold: u64,
    /// Refuse a body larger than this outright.
    pub max_body: u64,
    /// Where spilled bodies go.
    pub spill_dir: PathBuf,
    pub request_timeout: Duration,
}

impl Default for HubbleConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            user_agent: DEFAULT_USER_AGENT.to_owned(),
            rps: DEFAULT_RPS,
            // p90 is 135 KiB, so this keeps ~9 repos in 10 off the disk while
            // capping what a worker can be holding.
            spill_threshold: 1 << 20,
            // The largest repo sampled was 87 MB; 512 MiB is a guard against a
            // pathological or hostile response, not a working limit.
            max_body: 512 << 20,
            spill_dir: std::env::temp_dir(),
            request_timeout: Duration::from_secs(300),
        }
    }
}

pub struct HubbleSource {
    cfg: HubbleConfig,
    client: reqwest::Client,
    limiter: Arc<DefaultDirectRateLimiter>,
}

impl HubbleSource {
    pub fn new(cfg: HubbleConfig) -> Result<Self, SourceError> {
        let client = reqwest::Client::builder()
            .user_agent(&cfg.user_agent)
            .timeout(cfg.request_timeout)
            .build()
            .map_err(|e| SourceError::Transport(format!("client build: {e}")))?;
        let limiter = Arc::new(RateLimiter::direct(Quota::per_second(cfg.rps)));
        Ok(Self {
            cfg,
            client,
            limiter,
        })
    }

    /// Block until this source's token bucket allows another request.
    async fn wait(&self) {
        self.limiter.until_ready().await;
    }

    fn url(&self, path: &str) -> String {
        format!("{}/xrpc/{path}", self.cfg.base_url.trim_end_matches('/'))
    }

    /// Turn a response into a `SourceError` if it is not a success, carrying
    /// `Retry-After` through so the caller can honour it.
    fn check_status(resp: &reqwest::Response) -> Result<(), SourceError> {
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        Err(SourceError::Status {
            status: status.as_u16(),
            retry_after_secs: parse_retry_after(resp.headers()),
        })
    }

    async fn list_repos_once(
        &self,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<RepoPage, SourceError> {
        let mut url = url::Url::parse(&self.url("com.atproto.sync.listRepos"))
            .map_err(|e| SourceError::Decode(format!("bad base url: {e}")))?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("limit", &limit.min(MAX_PAGE_LIMIT).to_string());
            if let Some(c) = cursor {
                q.append_pair("cursor", c);
            }
        }

        self.wait().await;
        let resp = self
            .client
            .get(url.as_str())
            .send()
            .await
            .map_err(|e| SourceError::Transport(format!("listRepos: {e}")))?;
        Self::check_status(&resp)?;

        let body: ListReposBody = resp
            .json()
            .await
            .map_err(|e| SourceError::Decode(format!("listRepos body: {e}")))?;

        Ok(RepoPage {
            repos: body
                .repos
                .into_iter()
                .map(|r| RepoRef {
                    did: r.did,
                    rev: r.rev,
                    // hubble omits `active` for some rows; absent means active,
                    // matching how the lexicon treats it.
                    active: r.active.unwrap_or(true),
                    status: r.status,
                })
                .collect(),
            // An empty cursor string means "done", not "start over" -- a
            // distinction the current producer does not draw.
            cursor: body.cursor.filter(|c| !c.is_empty()),
        })
    }
}

impl RepoSource for HubbleSource {
    fn name(&self) -> &'static str {
        "hubble"
    }

    async fn list_repos(&self, cursor: Option<&str>, limit: u32) -> Result<RepoPage, SourceError> {
        let mut attempt = 0u32;
        loop {
            match self.list_repos_once(cursor, limit).await {
                Ok(page) => return Ok(page),
                Err(e) if attempt < LIST_MAX_RETRIES && classify(&e).is_transient() => {
                    attempt += 1;
                    let delay = super::source::retry_after_secs(&e)
                        .unwrap_or_else(|| backoff_secs(attempt));
                    tracing::warn!(
                        attempt,
                        max = LIST_MAX_RETRIES,
                        delay_secs = delay,
                        error = %e,
                        "hubble listRepos: transient failure, backing off"
                    );
                    sleep_secs(delay).await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    async fn fetch_repo(
        &self,
        did: &str,
        spill_threshold: u64,
        cap: u64,
    ) -> Result<RepoBody, SourceError> {
        let mut url = url::Url::parse(&self.url("com.atproto.sync.getRepo"))
            .map_err(|e| SourceError::Decode(format!("bad base url: {e}")))?;
        url.query_pairs_mut().append_pair("did", did);

        self.wait().await;
        let mut resp = self
            .client
            .get(url.as_str())
            // hubble also serves star-lite; we ask for CAR because that is what
            // `rsky-repo` reads.
            .header(reqwest::header::ACCEPT, "application/vnd.ipld.car")
            .send()
            .await
            .map_err(|e| SourceError::Transport(format!("getRepo {did}: {e}")))?;
        Self::check_status(&resp)?;

        // Stream rather than `bytes()`. The p99.9 repo is 24 MiB and the
        // largest sampled was 87 MB; buffering that whole tail per worker is
        // what makes the current backfiller's footprint a function of luck.
        let mut buf: Vec<u8> = Vec::new();
        let mut spill: Option<(tempfile::NamedTempFile, tokio::fs::File)> = None;
        let mut total: u64 = 0;

        loop {
            let chunk = resp
                .chunk()
                .await
                .map_err(|e| SourceError::Transport(format!("getRepo {did} body: {e}")))?;
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
                // Crossed the threshold: move what we have to disk and keep going.
                let tmp = tempfile::NamedTempFile::new_in(&self.cfg.spill_dir)
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
                buf.shrink_to_fit();
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
}

/// Per-DID metadata without transferring the repo.
///
/// hubble's own extension. Useful as cheap triage -- it reports the record
/// count and current rev, so "is this worth fetching" is answerable for a few
/// hundred bytes instead of a few hundred kilobytes.
#[derive(Debug, Clone, Deserialize)]
pub struct RepoInfo {
    pub did: String,
    #[serde(default)]
    pub archive: Option<RepoInfoArchive>,
    #[serde(default, rename = "syncState")]
    pub sync_state: Option<RepoInfoSyncState>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RepoInfoArchive {
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub records: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RepoInfoSyncState {
    #[serde(default)]
    pub rev: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
}

impl HubbleSource {
    pub async fn repo_info(&self, did: &str) -> Result<RepoInfo, SourceError> {
        let mut url = url::Url::parse(&self.url("blue.microcosm.hubble.getRepoInfo"))
            .map_err(|e| SourceError::Decode(format!("bad base url: {e}")))?;
        url.query_pairs_mut().append_pair("did", did);

        self.wait().await;
        let resp = self
            .client
            .get(url.as_str())
            .send()
            .await
            .map_err(|e| SourceError::Transport(format!("getRepoInfo {did}: {e}")))?;
        Self::check_status(&resp)?;
        resp.json()
            .await
            .map_err(|e| SourceError::Decode(format!("getRepoInfo body: {e}")))
    }
}

/// Convenience: whether a status string from enumeration means the archive will
/// never be fetchable, so the row can go straight to terminal instead of
/// spending a getRepo that will 400.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_sit_well_under_measured_capacity() {
        let cfg = HubbleConfig::default();
        // Measured knee was 119 repos/s at concurrency 16; 8 rps is ~7% of that.
        assert_eq!(cfg.rps.get(), 8);
        assert!(cfg.spill_threshold < cfg.max_body);
        assert!(cfg.user_agent.contains('@'), "must carry a contact address");
    }

    #[test]
    fn url_join_is_stable_with_or_without_trailing_slash() {
        let cfg = HubbleConfig {
            base_url: "https://example.test/".into(),
            ..HubbleConfig::default()
        };
        let s = HubbleSource::new(cfg).unwrap();
        assert_eq!(
            s.url("com.atproto.sync.listRepos"),
            "https://example.test/xrpc/com.atproto.sync.listRepos"
        );
    }

    #[test]
    fn inactive_repos_are_recognised_as_unfetchable() {
        assert!(!status_is_unfetchable(true, None));
        assert!(!status_is_unfetchable(true, Some("deleted")));
        assert!(status_is_unfetchable(false, Some("deleted")));
        assert!(status_is_unfetchable(false, Some("takendown")));
        assert!(status_is_unfetchable(false, None));
        // An unknown inactive status is worth one attempt rather than a
        // permanent write-off.
        assert!(!status_is_unfetchable(false, Some("something-new")));
    }

    #[test]
    fn list_repos_body_tolerates_hubble_and_relay_shapes() {
        // hubble: rev present, head empty, active present.
        let hubble = r#"{"repos":[{"did":"did:plc:a","rev":"3lido2","head":"","active":true}],
                         "cursor":"did:plc:a"}"#;
        let b: ListReposBody = serde_json::from_str(hubble).unwrap();
        assert_eq!(b.repos[0].rev, "3lido2");
        assert_eq!(b.cursor.as_deref(), Some("did:plc:a"));

        // A relay that omits active/status entirely still parses.
        let sparse = r#"{"repos":[{"did":"did:plc:b","rev":"3x"}],"cursor":null}"#;
        let b: ListReposBody = serde_json::from_str(sparse).unwrap();
        assert_eq!(b.repos[0].did, "did:plc:b");
        assert!(b.repos[0].active.is_none());
        assert!(b.cursor.is_none());
    }
}
