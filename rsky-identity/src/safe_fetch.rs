//! Outbound requests to addresses chosen by someone else: a DID document's
//! service endpoint, a handle's well-known path, a client's metadata URL.
//!
//! Every such request goes through a client whose name resolution keeps
//! only the addresses the [`NetworkPolicy`] permits, so the socket can only
//! ever reach a permitted address; literal addresses in URLs are checked
//! before any request; redirects are never followed by the transport and
//! only ever by [`SafeClient::get`] under the same checks; credentials in a
//! URL are refused; and bodies are read up to a bound.

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::redirect::Policy;
use reqwest::{Method, Response, StatusCode};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use url::Url;

/// Which destinations a fetch may reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkPolicy {
    /// Permit loopback addresses and the `localhost` name.
    pub allow_loopback: bool,
    /// Permit private, link-local, and other non-global addresses.
    pub allow_private: bool,
    /// Permit plain `http` in addition to `https`.
    pub allow_http: bool,
}

impl NetworkPolicy {
    /// Public addresses over https only: the policy for production.
    pub const PUBLIC: Self = Self {
        allow_loopback: false,
        allow_private: false,
        allow_http: false,
    };

    /// Anything reachable, for development against local services.
    pub const PERMISSIVE: Self = Self {
        allow_loopback: true,
        allow_private: true,
        allow_http: true,
    };

    /// Why `ip` may not be reached under this policy, if it may not.
    #[must_use]
    pub fn refusal(&self, ip: IpAddr) -> Option<&'static str> {
        let ip = match ip {
            IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
            v4 => v4,
        };
        let reason = match ip {
            IpAddr::V4(v4) => refusal_v4(v4),
            IpAddr::V6(v6) => refusal_v6(v6),
        };
        match reason {
            Some("loopback") if self.allow_loopback => None,
            Some("loopback") => Some("loopback"),
            Some(_) if self.allow_private => None,
            other => other,
        }
    }

    #[must_use]
    pub fn permits(&self, ip: IpAddr) -> bool {
        self.refusal(ip).is_none()
    }
}

fn refusal_v4(ip: Ipv4Addr) -> Option<&'static str> {
    let octets = ip.octets();
    if ip.is_loopback() {
        Some("loopback")
    } else if ip.is_unspecified() || ip.is_broadcast() {
        Some("unspecified")
    } else if ip.is_private() {
        Some("private")
    } else if ip.is_link_local() {
        Some("link-local")
    } else if octets[0] == 100 && (64..128).contains(&octets[1]) {
        Some("shared address space")
    } else if ip.is_multicast() || octets[0] >= 240 {
        Some("multicast or reserved")
    } else if ip.is_documentation() || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0) {
        Some("reserved")
    } else {
        None
    }
}

fn refusal_v6(ip: Ipv6Addr) -> Option<&'static str> {
    let segments = ip.segments();
    if ip.is_loopback() {
        Some("loopback")
    } else if ip.is_unspecified() {
        Some("unspecified")
    } else if segments[0] & 0xfe00 == 0xfc00 {
        Some("private")
    } else if segments[0] & 0xffc0 == 0xfe80 {
        Some("link-local")
    } else if ip.is_multicast() {
        Some("multicast or reserved")
    } else if segments[0] == 0x2001 && segments[1] == 0x0db8 {
        Some("reserved")
    } else if segments[0] == 0x64 && segments[1] == 0xff9b {
        // NAT64 carries an IPv4 address in its low bits
        let v4 = Ipv4Addr::new(
            (segments[6] >> 8) as u8,
            segments[6] as u8,
            (segments[7] >> 8) as u8,
            segments[7] as u8,
        );
        refusal_v4(v4)
    } else {
        None
    }
}

#[derive(Debug, Error)]
pub enum FetchError {
    #[error("{0} is not a valid url")]
    InvalidUrl(String),
    #[error("{url}: {reason}")]
    Refused { url: String, reason: String },
    #[error("too many redirects from {0}")]
    TooManyRedirects(String),
    #[error("response from {url} exceeds {limit} bytes")]
    TooLarge { url: String, limit: usize },
    #[error(transparent)]
    Transport(#[from] reqwest::Error),
}

const RESOLVER_REFUSAL: &str = "resolves to a";

/// A transport error that was really the resolver refusing every address,
/// rendered as the refusal it was.
fn classify(err: reqwest::Error, url: &Url) -> FetchError {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&err);
    while let Some(current) = source {
        let message = current.to_string();
        if let Some(at) = message.find(RESOLVER_REFUSAL) {
            return FetchError::Refused {
                url: url.to_string(),
                reason: message[at..].to_owned(),
            };
        }
        source = current.source();
    }
    FetchError::Transport(err)
}

/// Looks a host name up; the policy filters what it returns.
#[async_trait::async_trait]
pub trait Lookup: Send + Sync {
    async fn lookup(&self, host: &str) -> Result<Vec<IpAddr>, String>;
}

/// The system's resolver, through hickory.
pub struct SystemLookup {
    resolver: hickory_resolver::TokioAsyncResolver,
}

impl SystemLookup {
    #[must_use]
    pub fn new() -> Self {
        let resolver = hickory_resolver::TokioAsyncResolver::tokio_from_system_conf()
            .unwrap_or_else(|_| {
                hickory_resolver::TokioAsyncResolver::tokio(
                    hickory_resolver::config::ResolverConfig::default(),
                    hickory_resolver::config::ResolverOpts::default(),
                )
            });
        Self { resolver }
    }
}

impl Default for SystemLookup {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Lookup for SystemLookup {
    async fn lookup(&self, host: &str) -> Result<Vec<IpAddr>, String> {
        let answer = self
            .resolver
            .lookup_ip(host)
            .await
            .map_err(|e| e.to_string())?;
        Ok(answer.iter().collect())
    }
}

/// The resolver reqwest uses: every address a name resolves to is checked
/// against the policy before the transport sees it, so a name that points
/// at a forbidden address never yields a connection.
pub struct SafeResolver {
    policy: NetworkPolicy,
    lookup: Arc<dyn Lookup>,
}

impl SafeResolver {
    #[must_use]
    pub fn new(policy: NetworkPolicy, lookup: Arc<dyn Lookup>) -> Self {
        Self { policy, lookup }
    }

    /// The permitted addresses of `host`; an error when none is.
    pub async fn permitted(&self, host: &str) -> Result<Vec<IpAddr>, String> {
        let addresses = self.lookup.lookup(host).await?;
        let permitted: Vec<IpAddr> = addresses
            .iter()
            .copied()
            .filter(|ip| self.policy.permits(*ip))
            .collect();
        if permitted.is_empty() {
            let refused = addresses
                .iter()
                .filter_map(|ip| self.policy.refusal(*ip))
                .next()
                .unwrap_or("no address");
            return Err(format!("{host} {RESOLVER_REFUSAL} {refused} address"));
        }
        Ok(permitted)
    }
}

impl Resolve for SafeResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        let policy = self.policy;
        let lookup = self.lookup.clone();
        Box::pin(async move {
            let resolver = SafeResolver { policy, lookup };
            let addresses = resolver.permitted(&host).await?;
            let addrs: Addrs = Box::new(
                addresses
                    .into_iter()
                    .map(|ip| SocketAddr::new(ip, 0))
                    .collect::<Vec<_>>()
                    .into_iter(),
            );
            Ok(addrs)
        })
    }
}

/// How [`SafeClient::get`] treats a redirect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Redirects {
    /// Return the 3xx response as it is; the caller decides.
    None,
    /// Follow up to this many hops, checking every target.
    Follow(u8),
}

/// A client whose every request is bound by a [`NetworkPolicy`].
#[derive(Clone)]
pub struct SafeClient {
    policy: NetworkPolicy,
    resolver: Arc<SafeResolver>,
    client: reqwest::Client,
}

impl std::fmt::Debug for SafeClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SafeClient")
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl SafeClient {
    /// A client resolving through the system resolver.
    pub fn new(policy: NetworkPolicy, timeout: Duration) -> Result<Self, FetchError> {
        Self::with_lookup(policy, timeout, Arc::new(SystemLookup::new()))
    }

    /// A client resolving through `lookup`.
    pub fn with_lookup(
        policy: NetworkPolicy,
        timeout: Duration,
        lookup: Arc<dyn Lookup>,
    ) -> Result<Self, FetchError> {
        let resolver = Arc::new(SafeResolver::new(policy, lookup));
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(Policy::none())
            .dns_resolver(resolver.clone())
            .build()?;
        Ok(Self {
            policy,
            resolver,
            client,
        })
    }

    /// A builder for a transport with the same resolution and no redirects,
    /// for callers that need their own default headers or timeouts. Check
    /// every URL sent through it with [`SafeClient::check`].
    pub fn builder(&self) -> reqwest::ClientBuilder {
        reqwest::Client::builder()
            .redirect(Policy::none())
            .dns_resolver(self.resolver.clone())
    }

    #[must_use]
    pub const fn policy(&self) -> NetworkPolicy {
        self.policy
    }

    /// The transport, for requests that need their own method, headers, or
    /// body. Check the URL with [`SafeClient::check`] first; the transport
    /// itself never follows a redirect and only connects to permitted
    /// addresses.
    #[must_use]
    pub const fn transport(&self) -> &reqwest::Client {
        &self.client
    }

    /// Refuses a URL the policy does not allow before anything is sent:
    /// wrong scheme, credentials, no host, or a literal address that is
    /// not permitted.
    pub fn check(&self, url: &Url) -> Result<(), FetchError> {
        let refuse = |reason: &str| FetchError::Refused {
            url: url.to_string(),
            reason: reason.to_owned(),
        };
        match url.scheme() {
            "https" => {}
            "http" if self.policy.allow_http => {}
            other => return Err(refuse(&format!("scheme {other} is not permitted"))),
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(refuse("credentials in the url are not permitted"));
        }
        let Some(host) = url.host() else {
            return Err(refuse("no host"));
        };
        match host {
            url::Host::Ipv4(ip) => {
                if let Some(reason) = self.policy.refusal(IpAddr::V4(ip)) {
                    return Err(refuse(&format!("{reason} address")));
                }
            }
            url::Host::Ipv6(ip) => {
                if let Some(reason) = self.policy.refusal(IpAddr::V6(ip)) {
                    return Err(refuse(&format!("{reason} address")));
                }
            }
            url::Host::Domain(name) => {
                let loopback_name = name.eq_ignore_ascii_case("localhost")
                    || name.to_ascii_lowercase().ends_with(".localhost");
                if loopback_name && !self.policy.allow_loopback {
                    return Err(refuse("loopback name"));
                }
            }
        }
        Ok(())
    }

    /// Parses and checks `url`.
    pub fn checked(&self, url: &str) -> Result<Url, FetchError> {
        let parsed = Url::parse(url).map_err(|_| FetchError::InvalidUrl(url.to_owned()))?;
        self.check(&parsed)?;
        Ok(parsed)
    }

    /// A GET whose redirects, when followed, are checked hop by hop and
    /// never carry a body or credentials.
    pub async fn get(&self, url: Url, redirects: Redirects) -> Result<Response, FetchError> {
        let mut url = url;
        let mut hops = 0u8;
        loop {
            self.check(&url)?;
            let response = self
                .client
                .request(Method::GET, url.clone())
                .send()
                .await
                .map_err(|err| classify(err, &url))?;
            if !response.status().is_redirection() {
                return Ok(response);
            }
            let Redirects::Follow(max) = redirects else {
                return Ok(response);
            };
            if hops >= max {
                return Err(FetchError::TooManyRedirects(url.to_string()));
            }
            let Some(location) = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
            else {
                return Ok(response);
            };
            url = url
                .join(location)
                .map_err(|_| FetchError::InvalidUrl(location.to_owned()))?;
            hops += 1;
        }
    }

    /// Reads a body up to `limit` bytes.
    pub async fn read_bounded(
        response: Response,
        limit: usize,
    ) -> Result<(StatusCode, Vec<u8>), FetchError> {
        let url = response.url().to_string();
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|length| length > limit as u64)
        {
            return Err(FetchError::TooLarge { url, limit });
        }
        let mut response = response;
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if body.len() + chunk.len() > limit {
                return Err(FetchError::TooLarge { url, limit });
            }
            body.extend_from_slice(&chunk);
        }
        Ok((status, body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A lookup answering from a table, one answer per call in order.
    struct TableLookup {
        answers: Mutex<HashMap<String, Vec<Vec<IpAddr>>>>,
        calls: AtomicUsize,
    }

    impl TableLookup {
        fn new(entries: &[(&str, Vec<Vec<IpAddr>>)]) -> Arc<Self> {
            Arc::new(Self {
                answers: Mutex::new(
                    entries
                        .iter()
                        .map(|(host, answers)| ((*host).to_owned(), answers.clone()))
                        .collect(),
                ),
                calls: AtomicUsize::new(0),
            })
        }
    }

    #[async_trait::async_trait]
    impl Lookup for TableLookup {
        async fn lookup(&self, host: &str) -> Result<Vec<IpAddr>, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut answers = self.answers.lock().unwrap();
            let Some(queue) = answers.get_mut(host) else {
                return Err(format!("{host}: no such host"));
            };
            if queue.len() > 1 {
                Ok(queue.remove(0))
            } else {
                queue.first().cloned().ok_or_else(|| "empty".to_owned())
            }
        }
    }

    /// A one-shot HTTP/1.1 server on loopback counting accepted
    /// connections; the socket-level proof of what was reached.
    struct Listener {
        port: u16,
        accepted: Arc<AtomicUsize>,
    }

    impl Listener {
        async fn serve(status: u16, headers: &'static str, body: &'static str) -> Self {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let accepted = Arc::new(AtomicUsize::new(0));
            let counter = accepted.clone();
            tokio::spawn(async move {
                loop {
                    let Ok((mut socket, _)) = listener.accept().await else {
                        break;
                    };
                    counter.fetch_add(1, Ordering::SeqCst);
                    let mut buf = vec![0u8; 4096];
                    let _ = socket.read(&mut buf).await;
                    let response = format!(
                        "HTTP/1.1 {status} X\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                }
            });
            Self { port, accepted }
        }

        /// Serves a body delimited by the connection closing, with no
        /// declared length.
        async fn serve_unsized(body: &'static str) -> Self {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let accepted = Arc::new(AtomicUsize::new(0));
            let counter = accepted.clone();
            tokio::spawn(async move {
                while let Ok((mut socket, _)) = listener.accept().await {
                    counter.fetch_add(1, Ordering::SeqCst);
                    let mut buf = vec![0u8; 4096];
                    let _ = socket.read(&mut buf).await;
                    let response = format!("HTTP/1.1 200 X\r\nConnection: close\r\n\r\n{body}");
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                }
            });
            Self { port, accepted }
        }

        fn count(&self) -> usize {
            self.accepted.load(Ordering::SeqCst)
        }
    }

    fn v4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    const LOOPBACK_ONLY: NetworkPolicy = NetworkPolicy {
        allow_loopback: true,
        allow_private: false,
        allow_http: true,
    };

    #[test]
    fn the_policy_classifies_every_range() {
        let public = NetworkPolicy::PUBLIC;
        for (ip, reason) in [
            ("127.0.0.1", "loopback"),
            ("127.255.255.254", "loopback"),
            ("::1", "loopback"),
            ("::ffff:127.0.0.1", "loopback"),
            ("0.0.0.0", "unspecified"),
            ("255.255.255.255", "unspecified"),
            ("::", "unspecified"),
            ("10.0.0.1", "private"),
            ("172.16.5.5", "private"),
            ("192.168.1.1", "private"),
            ("fd00::1", "private"),
            ("169.254.169.254", "link-local"),
            ("fe80::1", "link-local"),
            ("100.64.0.1", "shared address space"),
            ("224.0.0.1", "multicast or reserved"),
            ("240.0.0.1", "multicast or reserved"),
            ("ff02::1", "multicast or reserved"),
            ("192.0.2.1", "reserved"),
            ("198.51.100.1", "reserved"),
            ("192.0.0.1", "reserved"),
            ("2001:db8::1", "reserved"),
            ("64:ff9b::a00:1", "private"),
            ("64:ff9b::7f00:1", "loopback"),
        ] {
            let ip: IpAddr = ip.parse().unwrap();
            assert_eq!(public.refusal(ip), Some(reason), "{ip}");
            assert!(!public.permits(ip));
        }
        for ip in ["1.1.1.1", "8.8.8.8", "2606:4700::1111", "64:ff9b::101:101"] {
            let ip: IpAddr = ip.parse().unwrap();
            assert_eq!(public.refusal(ip), None, "{ip}");
        }
        assert!(LOOPBACK_ONLY.permits("127.0.0.1".parse().unwrap()));
        assert!(!LOOPBACK_ONLY.permits("10.0.0.1".parse().unwrap()));
        assert!(NetworkPolicy::PERMISSIVE.permits("10.0.0.1".parse().unwrap()));
        assert!(NetworkPolicy::PERMISSIVE.permits("127.0.0.1".parse().unwrap()));
        // a private address stays refused when only loopback is allowed
        assert_eq!(
            LOOPBACK_ONLY.refusal("169.254.1.1".parse().unwrap()),
            Some("link-local")
        );
    }

    #[tokio::test]
    async fn a_name_pointing_at_a_forbidden_address_never_connects() {
        let listener = Listener::serve(200, "", "hello").await;
        let lookup = TableLookup::new(&[("evil.test", vec![vec![v4(127, 0, 0, 1)]])]);
        let client = SafeClient::with_lookup(
            NetworkPolicy {
                allow_http: true,
                ..NetworkPolicy::PUBLIC
            },
            Duration::from_secs(5),
            lookup.clone(),
        )
        .unwrap();
        let url = Url::parse(&format!("http://evil.test:{}/", listener.port)).unwrap();
        let err = client.get(url.clone(), Redirects::None).await.unwrap_err();
        assert!(err.to_string().contains("loopback"), "{err}");
        assert_eq!(listener.count(), 0, "the socket was never opened");
        assert_eq!(lookup.calls.load(Ordering::SeqCst), 1);

        // the same name is reachable when the policy allows loopback
        let client =
            SafeClient::with_lookup(LOOPBACK_ONLY, Duration::from_secs(5), lookup.clone()).unwrap();
        let response = client.get(url, Redirects::None).await.unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(listener.count(), 1);
        assert_eq!(client.policy(), LOOPBACK_ONLY);
        assert!(client.transport().get("http://evil.test/").build().is_ok());
        assert!(format!("{client:?}").contains("allow_loopback: true"));
        // a transport built from the builder resolves the same way
        let own = client.builder().build().unwrap();
        let response = own
            .get(format!("http://evil.test:{}/", listener.port))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(listener.count(), 2);
    }

    #[tokio::test]
    async fn an_address_that_changes_after_the_first_lookup_is_refused_next_time() {
        let listener = Listener::serve(200, "", "ok").await;
        // first answer loopback (allowed), then a private address
        let lookup = TableLookup::new(&[(
            "flip.test",
            vec![vec![v4(127, 0, 0, 1)], vec![v4(10, 0, 0, 1)]],
        )]);
        let client =
            SafeClient::with_lookup(LOOPBACK_ONLY, Duration::from_secs(5), lookup).unwrap();
        let url = Url::parse(&format!("http://flip.test:{}/", listener.port)).unwrap();
        assert_eq!(
            client
                .get(url.clone(), Redirects::None)
                .await
                .unwrap()
                .status(),
            200
        );
        assert_eq!(listener.count(), 1);
        let err = client.get(url, Redirects::None).await.unwrap_err();
        assert!(err.to_string().contains("private"), "{err}");
        assert_eq!(listener.count(), 1, "no second connection");
        // a name that mixes permitted and refused addresses keeps only the permitted
        let mixed =
            TableLookup::new(&[("mixed.test", vec![vec![v4(10, 0, 0, 1), v4(127, 0, 0, 1)]])]);
        let resolver = SafeResolver::new(LOOPBACK_ONLY, mixed);
        assert_eq!(
            resolver.permitted("mixed.test").await.unwrap(),
            vec![v4(127, 0, 0, 1)]
        );
        assert!(resolver
            .permitted("unknown.test")
            .await
            .unwrap_err()
            .contains("no such host"));
    }

    #[tokio::test]
    async fn literal_addresses_and_urls_are_checked_before_any_request() {
        let listener = Listener::serve(200, "", "ok").await;
        let lookup = TableLookup::new(&[]);
        let client = SafeClient::with_lookup(
            NetworkPolicy {
                allow_http: true,
                ..NetworkPolicy::PUBLIC
            },
            Duration::from_secs(5),
            lookup.clone(),
        )
        .unwrap();
        for url in [
            format!("http://127.0.0.1:{}/", listener.port),
            format!("http://[::1]:{}/", listener.port),
            format!("http://localhost:{}/", listener.port),
            format!("http://api.localhost:{}/", listener.port),
            "http://10.0.0.1/".to_owned(),
            "http://[fe80::1]/".to_owned(),
            "http://user:secret@example.com/".to_owned(),
            "http://user@example.com/".to_owned(),
            "ftp://example.com/".to_owned(),
            "data:text/plain,hi".to_owned(),
        ] {
            let err = client.checked(&url).unwrap_err();
            assert!(matches!(err, FetchError::Refused { .. }), "{url}: {err}");
        }
        assert!(matches!(
            client.checked("not a url").unwrap_err(),
            FetchError::InvalidUrl(_)
        ));
        assert_eq!(listener.count(), 0);
        assert_eq!(lookup.calls.load(Ordering::SeqCst), 0);
        // https is required unless the policy allows http
        let strict =
            SafeClient::with_lookup(NetworkPolicy::PUBLIC, Duration::from_secs(5), lookup).unwrap();
        let err = strict.checked("http://example.com/").unwrap_err();
        assert!(err.to_string().contains("scheme http"), "{err}");
        assert!(strict.checked("https://example.com/").is_ok());
        assert!(SafeClient::new(NetworkPolicy::PUBLIC, Duration::from_secs(1)).is_ok());
        assert!(SystemLookup::default().lookup("localhost").await.is_ok());
    }

    #[tokio::test]
    async fn redirects_are_followed_only_by_choice_and_only_to_permitted_targets() {
        let target = Listener::serve(200, "", "landed").await;
        let hop_two = Listener::serve(302, "Location: http://10.0.0.1/secret\r\n", "").await;
        let forbidden_hop = hop_two.port;
        let allowed_hop = target.port;
        let lookup = TableLookup::new(&[
            ("start.test", vec![vec![v4(127, 0, 0, 1)]]),
            ("target.test", vec![vec![v4(127, 0, 0, 1)]]),
            ("evil.test", vec![vec![v4(10, 0, 0, 1)]]),
        ]);
        let client =
            SafeClient::with_lookup(LOOPBACK_ONLY, Duration::from_secs(5), lookup.clone()).unwrap();

        // a redirect to a permitted host is followed when asked
        let start = Listener::serve(
            302,
            Box::leak(format!("Location: http://target.test:{allowed_hop}/x\r\n").into_boxed_str()),
            "",
        )
        .await;
        let url = Url::parse(&format!("http://start.test:{}/", start.port)).unwrap();
        let untouched = client.get(url.clone(), Redirects::None).await.unwrap();
        assert_eq!(untouched.status(), 302);
        assert_eq!(target.count(), 0, "not followed unless asked");
        let followed = client.get(url.clone(), Redirects::Follow(3)).await.unwrap();
        assert_eq!(followed.status(), 200);
        assert_eq!(followed.text().await.unwrap(), "landed");
        assert_eq!(target.count(), 1);

        // a hop to a forbidden address is refused with no connection
        let url = Url::parse(&format!("http://start.test:{forbidden_hop}/")).unwrap();
        let err = client.get(url, Redirects::Follow(3)).await.unwrap_err();
        assert!(err.to_string().contains("private address"), "{err}");
        // a hop to a name resolving to a forbidden address is refused too
        let by_name = Listener::serve(302, "Location: http://evil.test/\r\n", "").await;
        let url = Url::parse(&format!("http://start.test:{}/", by_name.port)).unwrap();
        let err = client.get(url, Redirects::Follow(3)).await.unwrap_err();
        assert!(err.to_string().contains("private"), "{err}");

        // the hop limit is enforced, a relative Location is resolved, and a
        // redirect without a Location is returned as it is
        let looping = Listener::serve(302, "Location: /again\r\n", "").await;
        let url = Url::parse(&format!("http://start.test:{}/", looping.port)).unwrap();
        let err = client.get(url, Redirects::Follow(2)).await.unwrap_err();
        assert!(matches!(err, FetchError::TooManyRedirects(_)), "{err}");
        assert_eq!(looping.count(), 3);
        let bare = Listener::serve(302, "", "").await;
        let url = Url::parse(&format!("http://start.test:{}/", bare.port)).unwrap();
        assert_eq!(
            client
                .get(url, Redirects::Follow(2))
                .await
                .unwrap()
                .status(),
            302
        );
        let bad_location = Listener::serve(302, "Location: http://[::zz/\r\n", "").await;
        let url = Url::parse(&format!("http://start.test:{}/", bad_location.port)).unwrap();
        assert!(matches!(
            client.get(url, Redirects::Follow(2)).await.unwrap_err(),
            FetchError::InvalidUrl(_)
        ));
    }

    #[tokio::test]
    async fn bodies_are_read_up_to_the_bound() {
        let small = Listener::serve(200, "", "0123456789").await;
        let lookup = TableLookup::new(&[("host.test", vec![vec![v4(127, 0, 0, 1)]])]);
        let client =
            SafeClient::with_lookup(LOOPBACK_ONLY, Duration::from_secs(5), lookup).unwrap();
        let url = Url::parse(&format!("http://host.test:{}/", small.port)).unwrap();
        let response = client.get(url.clone(), Redirects::None).await.unwrap();
        let (status, body) = SafeClient::read_bounded(response, 10).await.unwrap();
        assert_eq!(
            (status.as_u16(), body.as_slice()),
            (200, b"0123456789".as_slice())
        );
        let response = client.get(url, Redirects::None).await.unwrap();
        let err = SafeClient::read_bounded(response, 9).await.unwrap_err();
        assert!(
            matches!(err, FetchError::TooLarge { limit: 9, .. }),
            "{err}"
        );
        // a declared length over the bound is refused before reading
        let declared = Listener::serve(200, "", "0123456789").await;
        let url = Url::parse(&format!("http://host.test:{}/", declared.port)).unwrap();
        let response = client.get(url, Redirects::None).await.unwrap();
        assert!(matches!(
            SafeClient::read_bounded(response, 5).await.unwrap_err(),
            FetchError::TooLarge { limit: 5, .. }
        ));
        // without a declared length the bound applies as the body arrives
        let unsized_body = Listener::serve_unsized("0123456789").await;
        let url = Url::parse(&format!("http://host.test:{}/", unsized_body.port)).unwrap();
        let response = client.get(url.clone(), Redirects::None).await.unwrap();
        assert!(matches!(
            SafeClient::read_bounded(response, 5).await.unwrap_err(),
            FetchError::TooLarge { limit: 5, .. }
        ));
        let response = client.get(url, Redirects::None).await.unwrap();
        let (_, body) = SafeClient::read_bounded(response, 10).await.unwrap();
        assert_eq!(body, b"0123456789");
        // a connection refused is an ordinary transport error
        let err = client
            .get(Url::parse("http://host.test:1/").unwrap(), Redirects::None)
            .await
            .unwrap_err();
        assert!(matches!(err, FetchError::Transport(_)), "{err}");
    }
}
