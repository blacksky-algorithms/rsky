//! [`RepoSource`] backed by hubble, the public whole-network mirror.
//!
//! hubble is the default source for every host we do not fetch directly. It
//! takes the DID straight, so there is no DID→PDS resolution, and it serves
//! archives for hosts that are offline, slow or gone.
//!
//! Measured from nyc3: 9 repos/s at concurrency 1 rising linearly to 119 at
//! concurrency 16, zero 429s across 15,311 requests, then a flat ~28 MB/s byte
//! ceiling. That ceiling is why the mushroom fleet is fetched directly instead:
//! 98% of the network through one 28 MB/s pipe is four days of transfer, and it
//! is somebody else's grant money.

use std::num::NonZeroU32;
use std::sync::Arc;

use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};
use serde::Deserialize;

use super::pds::parse_list_repos;
use super::source::{
    FetchLimits, RepoBody, RepoPage, RepoSource, SourceError, read_body, status_error,
};

/// Public instance. The docs host (`web.hubble.microcosm.blue`) is a different
/// service and does not serve XRPC.
pub const DEFAULT_BASE_URL: &str = "https://hubble.microcosm.blue";

/// Identifies this app to hubble, with a contact address. hubble's policy
/// requires one; PDSes appreciate one.
pub const DEFAULT_USER_AGENT: &str =
    "rsky-wintermute/0.10 (+https://blacksky.app; clinton@blacksky.app)";

/// `listRepos` caps at 1000.
pub const MAX_PAGE_LIMIT: u32 = 1000;

/// Default self-imposed request rate. Written as a `match` because `unwrap` is
/// not permitted here and this needs to be a `const`.
pub const DEFAULT_RPS: NonZeroU32 = match NonZeroU32::new(8) {
    Some(v) => v,
    None => NonZeroU32::MIN,
};

#[derive(Debug, Clone)]
pub struct HubbleConfig {
    pub base_url: String,
    pub user_agent: String,
    /// Requests per second, enforced by a token bucket.
    pub rps: NonZeroU32,
}

impl Default for HubbleConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            user_agent: DEFAULT_USER_AGENT.to_owned(),
            rps: DEFAULT_RPS,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HubbleSource {
    cfg: HubbleConfig,
    client: reqwest::Client,
    limiter: Arc<DefaultDirectRateLimiter>,
}

impl HubbleSource {
    pub fn new(cfg: HubbleConfig) -> Result<Self, SourceError> {
        let client = reqwest::Client::builder()
            .user_agent(&cfg.user_agent)
            .connect_timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| SourceError::Transport(format!("client build: {e}")))?;
        Ok(Self::with_client(cfg, client))
    }

    #[must_use]
    pub fn with_client(cfg: HubbleConfig, client: reqwest::Client) -> Self {
        let limiter = Arc::new(RateLimiter::direct(Quota::per_second(cfg.rps)));
        Self {
            cfg,
            client,
            limiter,
        }
    }

    /// Block until this source's token bucket allows another request.
    async fn wait(&self) {
        self.limiter.until_ready().await;
    }

    fn url(&self, path: &str) -> String {
        format!("{}/xrpc/{path}", self.cfg.base_url.trim_end_matches('/'))
    }

    /// Per-DID metadata without transferring the repo: record count, current
    /// rev, sync state. Cheap triage.
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
        if !resp.status().is_success() {
            return Err(status_error(resp).await);
        }
        resp.json()
            .await
            .map_err(|e| SourceError::Decode(format!("getRepoInfo body: {e}")))
    }
}

impl RepoSource for HubbleSource {
    fn name(&self) -> &'static str {
        "hubble"
    }

    async fn list_repos(
        &self,
        cursor: Option<String>,
        limit: u32,
    ) -> Result<RepoPage, SourceError> {
        let mut url = url::Url::parse(&self.url("com.atproto.sync.listRepos"))
            .map_err(|e| SourceError::Decode(format!("bad base url: {e}")))?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("limit", &limit.min(MAX_PAGE_LIMIT).to_string());
            if let Some(c) = &cursor {
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
        if !resp.status().is_success() {
            return Err(status_error(resp).await);
        }
        let body = resp
            .bytes()
            .await
            .map_err(|e| SourceError::Transport(format!("listRepos body: {e}")))?;
        parse_list_repos(&body)
    }

    async fn fetch_repo(&self, did: String, limits: FetchLimits) -> Result<RepoBody, SourceError> {
        let mut url = url::Url::parse(&self.url("com.atproto.sync.getRepo"))
            .map_err(|e| SourceError::Decode(format!("bad base url: {e}")))?;
        url.query_pairs_mut().append_pair("did", &did);

        self.wait().await;
        let resp = self
            .client
            .get(url.as_str())
            // hubble also serves star-lite; we ask for CAR because that is what
            // `rsky-repo` reads.
            .header(reqwest::header::ACCEPT, "application/vnd.ipld.car")
            .timeout(limits.request_timeout)
            .send()
            .await
            .map_err(|e| SourceError::Transport(format!("getRepo {did}: {e}")))?;
        if !resp.status().is_success() {
            return Err(status_error(resp).await);
        }
        read_body(
            resp,
            limits.spill_threshold,
            limits.max_body,
            &limits.spill_dir,
        )
        .await
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RepoInfo {
    pub did: String,
    #[serde(default)]
    pub pds: Option<String>,
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

#[cfg(test)]
mod tests {
    #![allow(clippy::significant_drop_tightening)]
    use super::*;
    use crate::backfiller::source::{Class, classify};

    #[test]
    fn config_defaults_carry_a_contact_and_sit_under_measured_capacity() {
        let cfg = HubbleConfig::default();
        assert!(cfg.rps.get() < 119);
        assert!(cfg.user_agent.contains('@'), "must carry a contact address");
        assert_eq!(cfg.base_url, DEFAULT_BASE_URL);
    }

    #[test]
    fn url_join_is_stable_with_or_without_trailing_slash() {
        let a = HubbleSource::with_client(
            HubbleConfig {
                base_url: "https://h.example/".into(),
                ..HubbleConfig::default()
            },
            reqwest::Client::new(),
        );
        let b = HubbleSource::with_client(
            HubbleConfig {
                base_url: "https://h.example".into(),
                ..HubbleConfig::default()
            },
            reqwest::Client::new(),
        );
        assert_eq!(a.url("x"), b.url("x"));
        assert_eq!(a.url("x"), "https://h.example/xrpc/x");
        assert_eq!(a.name(), "hubble");
    }

    #[tokio::test]
    async fn hubble_round_trip_against_a_fake_instance() {
        let mut server = mockito::Server::new_async().await;
        let _list = server
            .mock("GET", "/xrpc/com.atproto.sync.listRepos")
            .match_query(mockito::Matcher::UrlEncoded("limit".into(), "1000".into()))
            .with_status(200)
            .with_body(
                r#"{"repos":[{"did":"did:plc:a","rev":"3lx","head":""}],"cursor":"did:plc:a"}"#,
            )
            .create_async()
            .await;
        let _repo = server
            .mock("GET", "/xrpc/com.atproto.sync.getRepo")
            .match_query(mockito::Matcher::Any)
            .match_header("accept", "application/vnd.ipld.car")
            .with_status(200)
            .with_body(vec![9u8; 10])
            .create_async()
            .await;
        let _info = server
            .mock("GET", "/xrpc/blue.microcosm.hubble.getRepoInfo")
            .match_query(mockito::Matcher::UrlEncoded(
                "did".into(),
                "did:plc:a".into(),
            ))
            .with_status(200)
            .with_body(
                r#"{"did":"did:plc:a","pds":"https://x","archive":{"available":true,"records":482},
                    "syncState":{"state":"synchronized","rev":"3lx"}}"#,
            )
            .create_async()
            .await;
        let _limited = server
            .mock("GET", "/xrpc/blue.microcosm.hubble.getRepoInfo")
            .match_query(mockito::Matcher::UrlEncoded(
                "did".into(),
                "did:plc:slow".into(),
            ))
            .with_status(429)
            .with_header("retry-after", "5")
            .create_async()
            .await;

        let s = HubbleSource::new(HubbleConfig {
            base_url: server.url(),
            ..HubbleConfig::default()
        })
        .unwrap();

        let page = s.list_repos(None, 1000).await.unwrap();
        assert_eq!(page.cursor.as_deref(), Some("did:plc:a"), "a DID cursor");
        let body = s
            .fetch_repo("did:plc:a".into(), FetchLimits::default())
            .await
            .unwrap();
        assert_eq!(body.len(), 10);
        let info = s.repo_info("did:plc:a").await.unwrap();
        assert_eq!(info.archive.unwrap().records, Some(482));
        assert_eq!(info.sync_state.unwrap().rev.as_deref(), Some("3lx"));
        assert_eq!(info.pds.as_deref(), Some("https://x"));

        let err = s.repo_info("did:plc:slow").await.unwrap_err();
        assert_eq!(classify(&err), Class::RateLimited);
    }
}
