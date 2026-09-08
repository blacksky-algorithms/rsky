//! DID resolver for hubble-sync
//!
//! for now the trait exists for test stubbing, but could be exposed to apps if
//! they want more control. personally i find it tricky enough to get right that
//! i'm starting with it being just internalized.
//!
//! for example: did:web redirect responses probably should not be followed. in
//! reqwest, turning off redirects is a *Client*-level policy. so... we can't
//! actually reuse the reqwest client instances from elsewhere, since we want
//! redirects to be followed on those.
//!
//! important: bidirectional handle verification isn't implemented yet
//! (hubble-sync and hubble don't need handles).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use metrics::{counter, histogram};
use reqwest::{StatusCode, Url, header};
use tracing::debug;

use super::doc;
use crate::metrics::{
    IDENTITY_RESOLVE_DURATION_SECONDS, IDENTITY_RESOLVE_OUTCOMES_TOTAL,
    IDENTITY_RESOLVE_REQUESTS_TOTAL,
};
use crate::storage::repo::RepoIdentity;
use crate::{Did, DidMethod, HostRegistry};

const PLC_CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const PLC_REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const PLC_RETRY_AFTER: Duration = Duration::from_secs(3);

const WEB_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const WEB_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const WEB_IDLE_TIMEOUT: Duration = Duration::from_secs(10);
const WEB_RATELIMIT_COOLDOWN: Duration = Duration::from_secs(15);

const MAX_DOC_SIZE: usize = 256 * 1024; // 256KiB is probably high but hey

#[derive(Debug, Clone, thiserror::Error)]
pub enum ResolutionError {
    #[error("too soon since last refresh. time remaining: {0:?}")]
    TooSoon(Duration),
    #[error("unresolvable: {0}")]
    Unresolvable(String),
    #[error("network: {0}")]
    Network(String),
    #[error("body: {0}")]
    Body(String),
    #[error("rate limited, retry after {retry_after:?}")]
    RateLimited { retry_after: Option<Duration> },
    #[error("not found")]
    NotFound,
    #[error("failed to parse: {0}")]
    DidDocError(#[from] doc::DidDocError),
}

pub type ResolvedIdentity = Result<RepoIdentity, ResolutionError>;

pub trait Resolve: Send + Sync + 'static {
    fn resolve(&self, did: &Did, now: SystemTime) -> impl Future<Output = ResolvedIdentity> + Send;
}

pub struct HubbleSyncResolver {
    hosts: Arc<HostRegistry>,
    plc_client: reqwest::Client,
    web_client: reqwest::Client,
    plc_url: String,
    plc_backpressure: Mutex<Option<Instant>>,
}

impl HubbleSyncResolver {
    pub fn new(contact: &str, plc_url: &str, hosts: Arc<HostRegistry>) -> Self {
        let ua = format!(
            "hubble-sync v{} DID resolver, from @microcosm.blue. contact: {contact}",
            env!("CARGO_PKG_VERSION"),
        );

        let plc_client = reqwest::Client::builder()
            .user_agent(&ua)
            .connect_timeout(PLC_CONNECT_TIMEOUT)
            .timeout(PLC_REQUEST_TIMEOUT)
            .build()
            .expect("reqwest client build");
        // note: no https- or redirect- hardening for plc

        let web_client = reqwest::Client::builder()
            .user_agent(&ua)
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none()) // this might be wrong: https://bsky.app/profile/thisismissem.social/post/3mo742dvi6c2f
            .connect_timeout(WEB_CONNECT_TIMEOUT)
            .timeout(WEB_REQUEST_TIMEOUT)
            .pool_idle_timeout(WEB_IDLE_TIMEOUT)
            .pool_max_idle_per_host(1)
            .build()
            .expect("reqwest client build");

        Self {
            hosts,
            plc_client,
            web_client,
            plc_url: plc_url.trim_end_matches('/').to_string(),
            plc_backpressure: Default::default(),
        }
    }

    fn get_retry_after(resp: &reqwest::Response) -> Option<Duration> {
        let h = resp.headers().get(header::RETRY_AFTER)?;
        let Ok(v) = h.to_str() else {
            debug!(retry_after = ?h, "found retry-after, failed to get it as str");
            return None;
        };
        let Ok(n) = v.parse::<u64>() else {
            debug!(
                retry_after = v,
                "found retry-after, failed parse as integer seconds"
            );
            // could also try parsing with `httpdate` crate (http date also legal retry-after)
            return None;
        };
        Some(Duration::from_secs(n))
    }

    /// metrics-wrapped actual fetch
    async fn fetch_doc(
        client: &reqwest::Client,
        url: Url,
        method: &DidMethod,
    ) -> Result<Vec<u8>, ResolutionError> {
        let start = Instant::now();
        let res = client.get(url).send().await;

        let (out, status) = match res {
            Ok(r) => {
                let status = r.status().as_str().to_string();
                (Self::get_bytes(r).await, status)
            }
            Err(err) => {
                let o = Err(ResolutionError::Network(err.to_string()));
                (o, "transport".to_string())
            }
        };

        counter!(IDENTITY_RESOLVE_REQUESTS_TOTAL, "method" => method.name(), "status" => status)
            .increment(1);
        histogram!(IDENTITY_RESOLVE_DURATION_SECONDS, "method" => method.name())
            .record(start.elapsed().as_secs_f64());

        out
    }

    async fn get_bytes(mut resp: reqwest::Response) -> Result<Vec<u8>, ResolutionError> {
        let status = resp.status();

        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(ResolutionError::RateLimited {
                retry_after: Self::get_retry_after(&resp),
            });
        }

        if status == StatusCode::NOT_FOUND {
            return Err(ResolutionError::NotFound);
        }

        if !status.is_success() {
            return Err(ResolutionError::Network(format!("HTTP {status}")));
        }

        let mut out = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| ResolutionError::Body(e.to_string()))?
        {
            let next_size = out.len() + chunk.len();
            if next_size > MAX_DOC_SIZE {
                return Err(ResolutionError::Body(format!(
                    "body size (at at elast {next_size}) larger than MAX_DOC_SIZE ({MAX_DOC_SIZE})"
                )));
            }
            out.extend_from_slice(&chunk)
        }

        Ok(out)
    }

    async fn resolve_plc(&self, did: &Did, now: SystemTime) -> ResolvedIdentity {
        let url: Url = format!("{}/{}", self.plc_url, did.as_str())
            .parse()
            .map_err(|e| ResolutionError::Unresolvable(format!("invalid in URL: {did:?}: {e}")))?;

        let raw = Self::fetch_doc(&self.plc_client, url, &did.method()).await?;
        Ok(doc::parse(raw, did, &self.hosts, now)?)
    }

    async fn resolve_web(&self, did: &Did, now: SystemTime) -> ResolvedIdentity {
        let host = did
            .as_str()
            .strip_prefix("did:web:")
            .unwrap_or_else(|| panic!("BUG: resolve_web called with non-did-web: {did:?}"));

        if host.is_empty() {
            return Err(ResolutionError::Unresolvable(format!(
                "did:web must have a host: {did:?}"
            )));
        }
        // TODO: allow localhost (with optional port) in dev environment only
        if host.contains(':') || host.contains("%3A") {
            return Err(ResolutionError::Unresolvable(format!(
                "did:web cannot contain a port or path segments: {did:?}"
            )));
        }
        if host.ends_with(".arpa") {
            // TODO other disallowed domains
            return Err(ResolutionError::Unresolvable(format!(
                "did:web cannot use this domain: {did:?}"
            )));
        }
        if host.contains('/') {
            return Err(ResolutionError::Unresolvable(format!(
                "did-web cannot contain slashes: {did:?}"
            )));
        }
        // TODO: handle any other url-encoding concerns???

        let url: Url = format!("https://{host}/.well-known/did.json")
            .parse()
            .map_err(|e| ResolutionError::Unresolvable(format!("invalid in URL: {did:?}: {e}")))?;

        let raw = Self::fetch_doc(&self.web_client, url, &did.method()).await?;
        Ok(doc::parse(raw, did, &self.hosts, now)?)
    }

    async fn plc_limit(&self) {
        let until = {
            // lock *must* drop immediately (holding over an await is death)
            *self.plc_backpressure.lock().expect("plc backpressure")
        };
        if let Some(u) = until
            && u > Instant::now()
        {
            // TODO: jitter? we might be bursty after this
            // or add a governor limiter as well?
            tokio::time::sleep_until(u.into()).await;
        }
    }

    fn push_plc_limit(&self, until: Instant) {
        let mut guard = self.plc_backpressure.lock().expect("plc backpressure");
        *guard = match *guard {
            None => Some(until),
            Some(existing) if until > existing => Some(until),
            _ => *guard,
        };
    }
}

impl Resolve for HubbleSyncResolver {
    async fn resolve(&self, did: &Did, now: SystemTime) -> ResolvedIdentity {
        let res = match did.method() {
            DidMethod::Plc => {
                self.plc_limit().await;
                let res = self.resolve_plc(did, now).await;

                // sneak in an immediate wait+retry if rate-limited
                if let Err(ResolutionError::RateLimited { retry_after }) = res {
                    let after = retry_after.unwrap_or(PLC_RETRY_AFTER);
                    self.push_plc_limit(Instant::now() + after);

                    // note: reusing the earlier 'now' system time
                    self.plc_limit().await;
                    return self.resolve_plc(did, now).await;
                }

                res
            }
            DidMethod::Web => {
                let res = self.resolve_web(did, now).await;

                // sneak in an immediate wait+retry if rate-limited
                if let Err(ResolutionError::RateLimited { retry_after }) = res {
                    let after = retry_after.unwrap_or(WEB_RATELIMIT_COOLDOWN);
                    tokio::time::sleep(after).await;
                    return self.resolve_web(did, now).await;
                }

                res
            }
        };

        use ResolutionError as RE;
        let outcome: &'static str = match &res {
            Ok(_) => "ok",
            Err(RE::RateLimited { .. } | RE::TooSoon(_)) => "rate_limited",
            Err(RE::NotFound) => "not_found",
            Err(RE::Network(_) | RE::Body(_) | RE::DidDocError(_)) => "request",
            Err(RE::Unresolvable(_)) => "unresolvable",
        };
        counter!(IDENTITY_RESOLVE_OUTCOMES_TOTAL, "method" => did.method().name(), "outcome" => outcome)
            .increment(1);

        res
    }
}
