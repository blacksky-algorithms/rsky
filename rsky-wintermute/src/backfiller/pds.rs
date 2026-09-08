//! [`RepoSource`] backed by a PDS reached directly.
//!
//! Used for hosts matched by the [`HostPolicy`]'s direct patterns -- by default
//! Bluesky's mushroom fleet, which carries ~98% of the network behind ~90
//! hosts. Fetching those directly gives ~90 independent rate budgets instead of
//! one shared byte-capped pipe through hubble, and leaves hubble to serve the
//! long tail of small, slow or offline PDSes it exists for.
//!
//! One `PdsSource` per host. All clones share the host's token bucket, so the
//! self-imposed rate holds across every task fetching from that host.

use std::sync::Arc;

use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};
use serde::Deserialize;

use super::host::{HostLimits, Hostname};
use super::source::{
    FetchLimits, RepoBody, RepoPage, RepoRef, RepoSource, SourceError, read_body, status_error,
};

/// `listRepos` caps at 1000.
pub const MAX_PAGE_LIMIT: u32 = 1000;

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

/// Parse a `listRepos` body. Shared with the hubble source, whose response has
/// the same shape (hubble always sends an empty `head`, which we ignore).
pub(super) fn parse_list_repos(body: &[u8]) -> Result<RepoPage, SourceError> {
    let body: ListReposBody = serde_json::from_slice(body)
        .map_err(|e| SourceError::Decode(format!("listRepos body: {e}")))?;
    Ok(RepoPage {
        repos: body
            .repos
            .into_iter()
            .map(|r| RepoRef {
                did: r.did,
                rev: r.rev,
                // Absent means active, matching how the lexicon treats it.
                active: r.active.unwrap_or(true),
                status: r.status,
            })
            .collect(),
        // An empty cursor string means "done", not "start over".
        cursor: body.cursor.filter(|c| !c.is_empty()),
    })
}

#[derive(Debug, Clone)]
pub struct PdsSource {
    host: Hostname,
    base_url: String,
    client: reqwest::Client,
    limiter: Arc<DefaultDirectRateLimiter>,
}

impl PdsSource {
    /// `client` should carry an identifying user-agent; it is shared across
    /// hosts so connection pools are bounded process-wide.
    #[must_use]
    pub fn new(host: Hostname, client: reqwest::Client, limits: &HostLimits) -> Self {
        let limiter = Arc::new(RateLimiter::direct(Quota::per_second(limits.rps)));
        let base_url = host.base_url();
        Self {
            host,
            base_url,
            client,
            limiter,
        }
    }

    /// For tests: a source whose XRPC base is an arbitrary URL.
    #[must_use]
    pub fn with_base_url(mut self, base_url: &str) -> Self {
        base_url
            .trim_end_matches('/')
            .clone_into(&mut self.base_url);
        self
    }

    #[must_use]
    pub const fn host(&self) -> &Hostname {
        &self.host
    }

    /// Block until this host's token bucket allows another request.
    async fn wait(&self) {
        self.limiter.until_ready().await;
    }

    fn url(&self, path: &str) -> String {
        format!("{}/xrpc/{path}", self.base_url)
    }
}

impl RepoSource for PdsSource {
    fn name(&self) -> &str {
        self.host.as_str()
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
            .map_err(|e| SourceError::Transport(format!("listRepos {}: {e}", self.host)))?;
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

#[cfg(test)]
mod tests {
    #![allow(clippy::significant_drop_tightening)]
    use super::*;
    use crate::backfiller::source::{Class, classify};
    use std::num::NonZeroU32;

    fn limits() -> HostLimits {
        HostLimits {
            rps: NonZeroU32::new(1000).unwrap(),
            concurrency: 4,
            adaptive: true,
        }
    }

    fn source(base: &str) -> PdsSource {
        PdsSource::new(
            Hostname::new("pds.example.test").unwrap(),
            reqwest::Client::new(),
            &limits(),
        )
        .with_base_url(base)
    }

    #[test]
    fn list_repos_body_tolerates_pds_hubble_and_relay_shapes() {
        let pds = br#"{"cursor":"12345","repos":[
            {"did":"did:plc:a","head":"bafy","rev":"3lx","active":true},
            {"did":"did:plc:b","head":"bafy","rev":"3ly","active":false,"status":"deactivated"}]}"#;
        let page = parse_list_repos(pds).unwrap();
        assert_eq!(page.cursor.as_deref(), Some("12345"));
        assert_eq!(page.repos.len(), 2);
        assert!(page.repos[0].active);
        assert_eq!(page.repos[1].status.as_deref(), Some("deactivated"));

        let hubble = br#"{"repos":[{"did":"did:plc:c","rev":"3lz","head":""}],"cursor":""}"#;
        let page = parse_list_repos(hubble).unwrap();
        assert_eq!(page.cursor, None, "empty cursor is end-of-list");
        assert!(page.repos[0].active, "absent active means active");

        let relay = br#"{"repos":[{"did":"did:plc:d"}]}"#;
        let page = parse_list_repos(relay).unwrap();
        assert_eq!(page.repos[0].rev, "", "missing rev is empty, never fails");

        assert!(matches!(
            parse_list_repos(b"not json"),
            Err(SourceError::Decode(_))
        ));
    }

    #[test]
    fn name_is_the_hostname_and_base_url_is_https() {
        let s = PdsSource::new(
            Hostname::new("Morel.us-east.host.bsky.network").unwrap(),
            reqwest::Client::new(),
            &limits(),
        );
        assert_eq!(s.name(), "morel.us-east.host.bsky.network");
        assert_eq!(
            s.url("com.atproto.sync.getRepo"),
            "https://morel.us-east.host.bsky.network/xrpc/com.atproto.sync.getRepo"
        );
        assert_eq!(s.host().as_str(), "morel.us-east.host.bsky.network");
    }

    #[tokio::test]
    async fn list_and_fetch_round_trip_against_a_fake_pds() {
        let mut server = mockito::Server::new_async().await;
        let _list = server
            .mock("GET", "/xrpc/com.atproto.sync.listRepos")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("limit".into(), "1000".into()),
                mockito::Matcher::UrlEncoded("cursor".into(), "abc".into()),
            ]))
            .with_status(200)
            .with_body(r#"{"repos":[{"did":"did:plc:a","rev":"3lx"}],"cursor":"def"}"#)
            .create_async()
            .await;
        let _repo = server
            .mock("GET", "/xrpc/com.atproto.sync.getRepo")
            .match_query(mockito::Matcher::UrlEncoded(
                "did".into(),
                "did:plc:a".into(),
            ))
            .with_status(200)
            .with_body(vec![1u8, 2, 3])
            .create_async()
            .await;
        let _missing = server
            .mock("GET", "/xrpc/com.atproto.sync.getRepo")
            .match_query(mockito::Matcher::UrlEncoded(
                "did".into(),
                "did:plc:gone".into(),
            ))
            .with_status(400)
            .with_body(r#"{"error":"RepoNotFound","message":"nope"}"#)
            .create_async()
            .await;

        let s = source(&server.url());
        let page = s.list_repos(Some("abc".into()), 5000).await.unwrap();
        assert_eq!(page.cursor.as_deref(), Some("def"));
        assert_eq!(page.repos[0].did, "did:plc:a");

        let body = s
            .fetch_repo("did:plc:a".into(), FetchLimits::default())
            .await
            .unwrap();
        assert_eq!(body.len(), 3);
        assert!(!body.spilled());

        let err = s
            .fetch_repo("did:plc:gone".into(), FetchLimits::default())
            .await
            .unwrap_err();
        assert_eq!(classify(&err), Class::Terminal);
    }

    #[tokio::test]
    async fn transport_failures_are_transient() {
        // Nothing listens here.
        let s = source("http://127.0.0.1:9");
        let err = s.list_repos(None, 10).await.unwrap_err();
        assert_eq!(classify(&err), Class::Transport);
        let err = s
            .fetch_repo("did:plc:a".into(), FetchLimits::default())
            .await
            .unwrap_err();
        assert_eq!(classify(&err), Class::Transport);
    }
}
