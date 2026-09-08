//! PDS host discovery: `com.atproto.sync.listHosts` on a relay.
//!
//! Every PDS worth fetching from directly has announced itself to the relay we
//! already subscribe to, and the relay reports each host's account count and
//! crawl status. That is enough to pick which hosts to fetch directly (the
//! mushrooms) and which to leave to hubble (everything small, and everything
//! the relay reports offline or banned).

use serde::Deserialize;

use super::host::Hostname;
use super::source::{SourceError, status_error};

/// `listHosts` caps at 1000.
const LIST_HOSTS_PAGE: u32 = 1000;
/// A relay that will not answer `listHosts` after this many tries is a
/// configuration problem, not a blip.
const MAX_ATTEMPTS: u32 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredHost {
    pub host: Hostname,
    pub account_count: u64,
    /// Relay-reported: `active`, `idle`, `offline`, `throttled`, `banned`.
    pub status: Option<String>,
}

impl DiscoveredHost {
    /// Whether the relay thinks this host is worth asking. Offline and banned
    /// hosts are left to hubble, which may still hold their archives.
    #[must_use]
    pub fn crawlable(&self) -> bool {
        !matches!(
            self.status.as_deref(),
            Some("offline" | "banned" | "throttled")
        )
    }
}

#[derive(Debug, Deserialize)]
struct ListHostsBody {
    hosts: Vec<ListHostsHost>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListHostsHost {
    hostname: String,
    #[serde(default, rename = "accountCount")]
    account_count: Option<u64>,
    #[serde(default)]
    status: Option<String>,
}

pub(super) fn parse_list_hosts(
    body: &[u8],
) -> Result<(Vec<DiscoveredHost>, Option<String>), SourceError> {
    let body: ListHostsBody = serde_json::from_slice(body)
        .map_err(|e| SourceError::Decode(format!("listHosts body: {e}")))?;
    let mut hosts = Vec::with_capacity(body.hosts.len());
    for h in body.hosts {
        match Hostname::new(&h.hostname) {
            Ok(host) => hosts.push(DiscoveredHost {
                host,
                account_count: h.account_count.unwrap_or(0),
                status: h.status,
            }),
            Err(e) => tracing::warn!(%e, "listHosts: skipping unparseable hostname"),
        }
    }
    Ok((hosts, body.cursor.filter(|c| !c.is_empty())))
}

/// Walk `listHosts` on `relay_base_url` (scheme included) to completion.
pub async fn list_hosts(
    client: &reqwest::Client,
    relay_base_url: &str,
) -> Result<Vec<DiscoveredHost>, SourceError> {
    let base = relay_base_url.trim_end_matches('/');
    let mut all = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut url = url::Url::parse(&format!("{base}/xrpc/com.atproto.sync.listHosts"))
            .map_err(|e| SourceError::Decode(format!("bad relay url: {e}")))?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("limit", &LIST_HOSTS_PAGE.to_string());
            if let Some(c) = &cursor {
                q.append_pair("cursor", c);
            }
        }

        let mut attempt = 0u32;
        let body = loop {
            attempt += 1;
            let result = async {
                let resp = client
                    .get(url.as_str())
                    .send()
                    .await
                    .map_err(|e| SourceError::Transport(format!("listHosts: {e}")))?;
                if !resp.status().is_success() {
                    return Err(status_error(resp).await);
                }
                resp.bytes()
                    .await
                    .map_err(|e| SourceError::Transport(format!("listHosts body: {e}")))
            }
            .await;
            match result {
                Ok(b) => break b,
                Err(e) if attempt < MAX_ATTEMPTS && super::source::classify(&e).is_transient() => {
                    let delay = super::source::backoff_secs(attempt);
                    tracing::warn!(attempt, delay_secs = delay, error = %e, "listHosts: retrying");
                    super::source::sleep_secs(delay).await;
                }
                Err(e) => return Err(e),
            }
        };

        let (hosts, next) = parse_list_hosts(&body)?;
        all.extend(hosts);
        match next {
            Some(c) if cursor.as_deref() != Some(c.as_str()) => cursor = Some(c),
            Some(_) => {
                tracing::warn!("listHosts: relay returned the cursor we sent; stopping");
                break;
            }
            None => break,
        }
    }
    tracing::info!(hosts = all.len(), relay = base, "listHosts complete");
    Ok(all)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::significant_drop_tightening)]
    use super::*;

    #[test]
    fn parses_relay_shape_and_skips_garbage_hostnames() {
        let body = br#"{"cursor":"9","hosts":[
            {"hostname":"lepista.us-west.host.bsky.network","accountCount":127324,"seq":1,"status":"active"},
            {"hostname":"","accountCount":1},
            {"hostname":"pds.example","status":"offline"}]}"#;
        let (hosts, cursor) = parse_list_hosts(body).unwrap();
        assert_eq!(cursor.as_deref(), Some("9"));
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].account_count, 127_324);
        assert!(hosts[0].crawlable());
        assert_eq!(hosts[1].account_count, 0);
        assert!(!hosts[1].crawlable());
        assert!(matches!(
            parse_list_hosts(b"[]"),
            Err(SourceError::Decode(_))
        ));
    }

    #[test]
    fn crawlable_by_status() {
        let mk = |s: Option<&str>| DiscoveredHost {
            host: Hostname::new("x.example").unwrap(),
            account_count: 1,
            status: s.map(str::to_owned),
        };
        assert!(mk(None).crawlable());
        assert!(mk(Some("active")).crawlable());
        assert!(mk(Some("idle")).crawlable());
        assert!(!mk(Some("offline")).crawlable());
        assert!(!mk(Some("banned")).crawlable());
        assert!(!mk(Some("throttled")).crawlable());
    }

    #[tokio::test]
    async fn walks_pages_and_stops_on_a_repeated_cursor() {
        let mut server = mockito::Server::new_async().await;
        let _p1 = server
            .mock("GET", "/xrpc/com.atproto.sync.listHosts")
            .match_query(mockito::Matcher::UrlEncoded("limit".into(), "1000".into()))
            .with_status(200)
            .with_body(r#"{"hosts":[{"hostname":"a.example","accountCount":5}],"cursor":"c1"}"#)
            .expect(1)
            .create_async()
            .await;
        let _p2 = server
            .mock("GET", "/xrpc/com.atproto.sync.listHosts")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("limit".into(), "1000".into()),
                mockito::Matcher::UrlEncoded("cursor".into(), "c1".into()),
            ]))
            .with_status(200)
            .with_body(r#"{"hosts":[{"hostname":"b.example"}],"cursor":"c1"}"#)
            .expect(1)
            .create_async()
            .await;
        let hosts = list_hosts(&reqwest::Client::new(), &(server.url() + "/"))
            .await
            .unwrap();
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[1].host.as_str(), "b.example");
    }

    #[tokio::test]
    async fn terminal_relay_errors_surface() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/xrpc/com.atproto.sync.listHosts")
            .match_query(mockito::Matcher::Any)
            .with_status(404)
            .create_async()
            .await;
        let err = list_hosts(&reqwest::Client::new(), &server.url())
            .await
            .unwrap_err();
        assert!(matches!(err, SourceError::Status { status: 404, .. }));
    }
}
