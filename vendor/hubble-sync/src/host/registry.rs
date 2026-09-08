use super::{
    DEFAULT_BSKY_HOST_CONCURRENCY, DEFAULT_BSKY_HOST_QPS, DEFAULT_HOST_CONCURRENCY,
    DEFAULT_HOST_QPS, DEFAULT_RELAY_HOST_CONCURRENCY, DEFAULT_RELAY_HOST_QPS, HOST_CONNECT_TIMEOUT,
    Host, Hostname, HostnameError, RedirectingClient, normalize_hostname,
};
use std::collections::HashMap;
use std::fmt;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use metrics::counter;
use tokio_util::sync::CancellationToken;

use crate::SyncScope;
use crate::config::UpstreamConfig;
use crate::metrics::HOST_REGISTRY_LOADS_TOTAL;

#[derive(Debug, Clone, Default)]
pub struct HostRegistryConfig {
    pub host_qps: Option<NonZeroU32>,
    pub host_bsky_qps: Option<NonZeroU32>,
    pub host_concurrency: Option<usize>,
    pub host_bsky_concurrency: Option<usize>,
}

pub struct HostRegistry {
    inner: Mutex<HashMap<Hostname, Weak<Host>>>,
    client: Arc<RedirectingClient>,
    host_qps: NonZeroU32,
    host_bsky_qps: NonZeroU32,
    host_concurrency: usize,
    host_bsky_concurrency: usize,
    upstream: UpstreamConfig,
    /// operator-allowlisted targets for forwarding Authorization across
    /// cross-origin redirects (normalized hostnames)
    auth_forward_hosts: Vec<Hostname>,
    sync_scope: SyncScope,
}

impl HostRegistry {
    pub fn new(
        config: HostRegistryConfig,
        upstream: UpstreamConfig,
        ua: &str,
        cancel: CancellationToken,
        sync_scope: SyncScope,
    ) -> Arc<Self> {
        let client = reqwest::Client::builder()
            .user_agent(ua)
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none()) // we handle (with limits)
            .connect_timeout(HOST_CONNECT_TIMEOUT)
            // no .timeout() since we vary it per-reqwest: apply tokio timeout externally
            .pool_idle_timeout(Some(Duration::from_secs(300))) // hold warm connections longer
            .tcp_keepalive(Some(Duration::from_secs(30))) // avoid some idle drops
            .http2_adaptive_window(true)
            .build()
            .expect("reqwest client build");
        // TODO: ssrf defence?

        let auth_forward_hosts = upstream
            .get_repo_token_forward_to
            .iter()
            .map(|h| {
                let normalized = normalize_hostname(h)
                    .unwrap_or_else(|e| panic!("invalid auth-forward hostname {h:?}: {e}"));
                Hostname(normalized.into())
            })
            .collect();

        Arc::new_cyclic(|weak_self| {
            let client = Arc::new(RedirectingClient::new(client, weak_self.clone(), cancel));
            Self {
                inner: Default::default(),
                client,
                host_qps: config.host_qps.unwrap_or(DEFAULT_HOST_QPS),
                host_bsky_qps: config.host_bsky_qps.unwrap_or(DEFAULT_BSKY_HOST_QPS),
                host_concurrency: config.host_concurrency.unwrap_or(DEFAULT_HOST_CONCURRENCY),
                host_bsky_concurrency: config
                    .host_bsky_concurrency
                    .unwrap_or(DEFAULT_BSKY_HOST_CONCURRENCY),
                upstream,
                auth_forward_hosts,
                sync_scope,
            }
        })
    }

    #[cfg(test)]
    pub fn new_default() -> Arc<Self> {
        Self::new(
            Default::default(),
            Default::default(),
            "",
            CancellationToken::new(),
            SyncScope::Everything,
        )
    }

    /// create or get an existing `Arc<Host>` for `name`
    ///
    /// if you need a Host, you go through here, so we don't duplicate stuff.
    pub fn get(&self, name: &str) -> Result<Arc<Host>, HostnameError> {
        let normalized = normalize_hostname(name)?;

        let mut map = self.inner.lock().expect("host registry lock");

        // happy path: existing host is alive
        if let Some(weak) = map.get(normalized.as_str())
            && let Some(host) = weak.upgrade()
        {
            counter!(HOST_REGISTRY_LOADS_TOTAL, "result" => "hit").increment(1);
            return Ok(host);
        }

        // unhappy path: initialize from scratch
        counter!(HOST_REGISTRY_LOADS_TOTAL, "result" => "miss").increment(1);
        let hostname = Hostname(normalized.into());

        let (mut qps, mut concurrency) = if hostname.is_bsky() {
            (self.host_bsky_qps, self.host_bsky_concurrency)
        } else {
            (self.host_qps, self.host_concurrency)
        };

        if hostname.as_str() == self.upstream.hostname {
            if let Some(q) = self.upstream.host_qps {
                qps = q;
            } else if self.upstream.kind.is_relay() {
                qps = DEFAULT_RELAY_HOST_QPS;
            }
            if let Some(c) = self.upstream.host_concurrency {
                concurrency = c;
            } else if self.upstream.kind.is_relay() {
                concurrency = DEFAULT_RELAY_HOST_CONCURRENCY;
            }
        }

        let host = Arc::new(Host::new(
            hostname.clone(),
            self.client.clone(),
            qps,
            concurrency,
            self.sync_scope,
        ));
        map.insert(hostname, Arc::downgrade(&host));
        Ok(host)
    }

    /// is `hostname` an operator-allowlisted target for forwarding the
    /// Authorization header across a cross-origin redirect?
    ///
    /// `hostname` comes from a resolved redirect URI, already lowercased by
    /// url parsing; entries were normalized at registry setup.
    pub(crate) fn forwards_auth_to(&self, hostname: &str) -> bool {
        self.auth_forward_hosts
            .iter()
            .any(|h| h.as_str() == hostname)
    }

    /// clear dropped Weak entries
    pub fn prune(&self) {
        self.inner
            .lock()
            .expect("host registry lock")
            .retain(|_, weak| weak.strong_count() > 0);
    }

    pub fn len(&self) -> usize {
        self.inner.lock().expect("host registry lock").len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().expect("host registry lock").is_empty()
    }

    #[cfg(test)]
    pub(crate) fn client_handle(&self) -> Arc<RedirectingClient> {
        self.client.clone()
    }
}

impl fmt::Debug for HostRegistry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("HostRegistry")
            .field("size", &self.inner.try_lock().map(|m| m.len()))
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry_forwarding_to(hosts: &[&str]) -> Arc<HostRegistry> {
        HostRegistry::new(
            Default::default(),
            UpstreamConfig {
                hostname: "pds.example.com".to_string(),
                get_repo_token_forward_to: hosts.iter().map(|h| h.to_string()).collect(),
                ..Default::default()
            },
            "",
            CancellationToken::new(),
            SyncScope::Everything,
        )
    }

    #[test]
    fn auth_forwards_only_to_allowlisted_hosts() {
        let reg = registry_forwarding_to(&["Files.Example.COM"]);
        assert!(
            reg.forwards_auth_to("files.example.com"),
            "allowlist entries are normalized at setup",
        );
        assert!(
            !reg.forwards_auth_to("pds.example.com"),
            "the upstream itself is not implicitly an auth-forward target",
        );
        assert!(!reg.forwards_auth_to("bad-example.com"));
    }

    #[test]
    fn default_config_forwards_auth_nowhere() {
        let reg = HostRegistry::new_default();
        assert!(!reg.forwards_auth_to("example.com"));
    }

    #[test]
    #[should_panic(expected = "invalid auth-forward hostname")]
    fn invalid_forward_hostname_fails_at_setup() {
        registry_forwarding_to(&["https://not-a-bare-hostname.example"]);
    }
}
