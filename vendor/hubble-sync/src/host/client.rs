//! http client below jacquard with redirect-aware host limiting

use std::pin::Pin;
use std::sync::{Arc, Weak};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use http::Request;
use jacquard_common::http_client::{HttpClient, HttpClientExt};
use jacquard_common::stream::{ByteStream, StreamError};
use metrics::{counter, histogram};
use tokio::sync::OwnedSemaphorePermit;
use tokio_util::sync::CancellationToken;

use super::{
    DEFAULT_MAX_NON_STREAM_BODY, DEFAULT_MAX_REDIRECTS, HOST_REQUEST_TIMEOUT, Host, HostRegistry,
    HostnameError, InBackoff,
};
use crate::CancelExt;
use crate::metrics::{HOST_LIMIT_WAIT_SECONDS, REQUESTS_SENT_TOTAL, RESPONSES_RECEIVED_TOTAL};

#[derive(Debug, thiserror::Error)]
pub(crate) enum HostClientError {
    #[error("host backing off: {0}")]
    Backoff(#[from] InBackoff),
    #[error("bad redirect: {0}")]
    BadRedirect(#[from] BadRedirect),
    #[error("transport: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("body too big")]
    BodyTooBig,
    #[error("cancelled")]
    Cancelled,
    #[error("request timed out")]
    Timeout,
    #[error("registry gone")]
    RegistryGone,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum BadRedirect {
    #[error("too many redirects")]
    TooManyRedirects,
    #[error("uri missing host")]
    MissingHost,
    #[error("bad redirect host: {0}")]
    Hostname(#[from] HostnameError),
    #[error("missing location header")]
    MissingLocation,
    #[error("bad location header: {0}")]
    BadLocation(#[from] http::header::ToStrError),
    #[error("bad location url: {0}")]
    BadLocationUrl(String),
}

/// manual redirect handling over a reqwest client
///
/// we track and limit resources by target host, so we need to be in-the-loop of
/// redirects when they cross hosts: releasing concurrency permits from the
/// original host and acquiring them for the redirect-target, etc.
///
/// there are a lot of details involved with handling redirects. we get some of
/// them right here, but review the implementation if you're doing anything
/// sensitive.
///
/// it would be nice to just implement a reqwest redirect policy, but that it
/// would get pretty tricky to make the per-host resource accounting work.
pub(crate) struct RedirectingClient {
    inner: reqwest::Client,
    registry: Weak<HostRegistry>,
    cancel: CancellationToken,
    max_redirects: usize,
}

impl RedirectingClient {
    pub(crate) fn new(
        inner: reqwest::Client,
        registry: Weak<HostRegistry>,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            inner,
            registry,
            cancel,
            max_redirects: DEFAULT_MAX_REDIRECTS,
        }
    }

    async fn follow_redirects(
        &self,
        mut req: Request<Vec<u8>>,
    ) -> Result<(reqwest::Response, OwnedSemaphorePermit, Arc<Host>, Duration), HostClientError>
    {
        let registry = self
            .registry
            .upgrade()
            .ok_or(HostClientError::RegistryGone)?;

        let mut limit_waited = Duration::ZERO;
        for _ in 0..self.max_redirects {
            let hostname = req.uri().host().ok_or(BadRedirect::MissingHost)?;
            let host = registry.get(hostname).map_err(BadRedirect::Hostname)?;

            let wait_start = Instant::now();
            let permit: OwnedSemaphorePermit = self
                .cancel
                .run(host.request_permit())
                .await
                .ok_or(HostClientError::Cancelled)??;
            let waited = wait_start.elapsed();
            limit_waited += waited;
            histogram!(HOST_LIMIT_WAIT_SECONDS).record(waited.as_secs_f64());

            let send = self
                .inner
                .request(req.method().clone(), req.uri().to_string())
                .headers(req.headers().clone())
                .body(req.body().clone())
                .send();

            counter!(REQUESTS_SENT_TOTAL).increment(1);
            let res = self
                .cancel
                .timeout(HOST_REQUEST_TIMEOUT, send)
                .await
                .ok_or(HostClientError::Cancelled)?
                .map_err(|_| HostClientError::Timeout)??;

            counter!(RESPONSES_RECEIVED_TOTAL, "status" => res.status().as_str().to_string())
                .increment(1);

            // note any rate-limit or transient errors from this host
            host.observe_response(res.status(), res.headers());

            if res.status().is_redirection() {
                let location = res
                    .headers()
                    .get(http::header::LOCATION)
                    .ok_or(BadRedirect::MissingLocation)?
                    .to_str()
                    .map_err(BadRedirect::BadLocation)?;

                drop(permit); // no body to read on redirect, so release now

                if matches!(
                    res.status(),
                    http::StatusCode::MOVED_PERMANENTLY
                        | http::StatusCode::FOUND
                        | http::StatusCode::SEE_OTHER
                ) {
                    // these redirects downgrade most methods to GETs
                    *req.body_mut() = vec![];
                    for header in &[
                        http::header::TRANSFER_ENCODING,
                        http::header::CONTENT_ENCODING,
                        http::header::CONTENT_TYPE,
                        http::header::CONTENT_LENGTH,
                    ] {
                        req.headers_mut().remove(header);
                    }
                    match req.method_mut() {
                        &mut http::Method::GET | &mut http::Method::HEAD => {}
                        m => *m = http::Method::GET,
                    }
                }

                let next = Self::resolve_redirect(req.uri(), location)?;
                if !Self::same_origin(req.uri(), &next) {
                    // strip sensitive headers on cross-origin redirect
                    for header in [
                        http::header::COOKIE,
                        http::header::PROXY_AUTHORIZATION,
                        http::header::WWW_AUTHENTICATE,
                    ] {
                        req.headers_mut().remove(header);
                    }
                    // Authorization too, unless the operator explicitly
                    // allowlisted the target host for auth-forwarding (eg.
                    // bridgy 301s authed getRepo across subdomains)
                    let forward_auth = next.host().is_some_and(|h| registry.forwards_auth_to(h));
                    if !forward_auth {
                        req.headers_mut().remove(http::header::AUTHORIZATION);
                    }
                }
                *req.uri_mut() = next;
                continue;
            }

            // forward permit to cover reading the body
            return Ok((res, permit, host, limit_waited));
        }
        Err(BadRedirect::TooManyRedirects.into())
    }

    fn same_origin(a: &http::Uri, b: &http::Uri) -> bool {
        fn port(uri: &http::Uri) -> Option<u16> {
            uri.port_u16().or(match uri.scheme_str() {
                Some("https") => Some(443),
                Some("http") => Some(80),
                _ => None,
            })
        }
        a.host() == b.host() && port(a) == port(b)
    }

    fn resolve_redirect(current: &http::Uri, location: &str) -> Result<http::Uri, BadRedirect> {
        let base = reqwest::Url::parse(&current.to_string())
            .map_err(|e| BadRedirect::BadLocationUrl(e.to_string()))?;

        let next = base
            .join(location) // absolute location replaces base
            .map_err(|e| BadRedirect::BadLocationUrl(e.to_string()))?;

        let next = next
            .as_str()
            .parse::<http::uri::Uri>()
            .map_err(|e| BadRedirect::BadLocationUrl(e.to_string()))?;

        Ok(next)
    }

    fn build_response(
        res: &reqwest::Response,
        resolved: Arc<Host>,
        limit_waited: Duration,
    ) -> http::response::Builder {
        let mut b = http::Response::builder().status(res.status());

        let h = b.headers_mut().expect("response builder ok");
        *h = res.headers().clone();

        let ext = b.extensions_mut().expect("response builder ok");
        ext.insert(ResolvedHost(resolved));
        ext.insert(LimitWaited(limit_waited));

        b
    }
}

impl HttpClient for RedirectingClient {
    type Error = HostClientError;

    /// normal (non-streaming) requests
    async fn send_http(
        &self,
        request: http::Request<Vec<u8>>,
    ) -> Result<http::Response<Vec<u8>>, Self::Error> {
        let (mut res, permit, resolved, limit_waited) = self.follow_redirects(request).await?;
        let builder = Self::build_response(&res, resolved, limit_waited);

        // note: for now, extensions are only exported back from hubble-sync (on
        // the onther side of jacquard) on the streaming path, because i'm not
        // sure if we can get access to the raw `http::Response` for normal
        // responses to access the extension.

        let mut body_bytes = Vec::new();
        while let Some(chunk) = res.chunk().await? {
            if body_bytes.len() + chunk.len() > DEFAULT_MAX_NON_STREAM_BODY {
                return Err(HostClientError::BodyTooBig);
            }
            body_bytes.extend_from_slice(&chunk);
        }
        drop(permit); // we have consumed the body

        Ok(builder.body(body_bytes).expect("response builder ok"))
    }
}

impl HttpClientExt for RedirectingClient {
    async fn send_http_streaming(
        &self,
        request: http::Request<Vec<u8>>,
    ) -> Result<http::Response<ByteStream>, HostClientError> {
        let (res, permit, resolved, limit_waited) = self.follow_redirects(request).await?;
        let builder = Self::build_response(&res, resolved, limit_waited);

        let inner = res
            .bytes_stream()
            .map(|r| r.map_err(StreamError::transport));

        let body_stream = ByteStream::new(PermitStream {
            inner: Box::pin(inner),
            permit, // carry into streaming body read
        });

        Ok(builder.body(body_stream).expect("response builder ok"))
    }

    async fn send_http_bidirectional<S>(
        &self,
        _parts: http::request::Parts,
        _body: S,
    ) -> Result<http::Response<ByteStream>, HostClientError>
    where
        S: n0_future::Stream<Item = Result<Bytes, StreamError>> + Send + 'static,
    {
        unimplemented!("wrapping streaming request bodies still TODO in hubble-sync");
    }
}

struct PermitStream {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes, StreamError>> + Send>>,
    #[expect(dead_code, reason = "resource limit that drops with stream")]
    permit: OwnedSemaphorePermit,
}

impl Stream for PermitStream {
    type Item = Result<Bytes, StreamError>;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().inner.as_mut().poll_next(cx)
    }
}

/// the host that actually served a response, after redirects resolved
#[derive(Clone)]
pub struct ResolvedHost(pub Arc<Host>);

/// total time a request spent queued at per-host limits (concurrency permit +
/// rate limiter), summed across redirect hops
#[derive(Clone, Copy)]
pub struct LimitWaited(pub Duration);

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use futures_util::{StreamExt, stream};
    use tokio::sync::Semaphore;

    use super::*;

    // The whole point of PermitStream: the host's concurrency permit must stay
    // held for as long as the streaming body reader is alive, so the per-host
    // download-concurrency limit actually covers the transfer (not just the
    // request headers). It releases on drop.

    #[tokio::test]
    async fn permit_held_while_stream_alive_released_on_drop() {
        let sem = Arc::new(Semaphore::new(1));
        let permit = sem.clone().acquire_owned().await.expect("permit");

        let stream = PermitStream {
            inner: Box::pin(stream::empty::<Result<Bytes, StreamError>>()),
            permit,
        };
        assert_eq!(sem.available_permits(), 0, "held while the stream exists");

        drop(stream);
        assert_eq!(
            sem.available_permits(),
            1,
            "released once the stream is dropped"
        );
    }

    #[tokio::test]
    async fn permit_survives_full_drain_until_dropped() {
        let sem = Arc::new(Semaphore::new(1));
        let permit = sem.clone().acquire_owned().await.expect("permit");

        let mut stream = PermitStream {
            inner: Box::pin(stream::iter(vec![
                Ok(Bytes::from_static(b"ab")),
                Ok(Bytes::from_static(b"cd")),
            ])),
            permit,
        };

        let mut body = Vec::new();
        while let Some(chunk) = stream.next().await {
            body.extend_from_slice(&chunk.expect("chunk"));
        }
        assert_eq!(body, b"abcd", "chunks forwarded unchanged");
        assert_eq!(
            sem.available_permits(),
            0,
            "draining the body does not release the permit — only dropping the reader does",
        );

        drop(stream);
        assert_eq!(sem.available_permits(), 1);
    }

    #[test]
    fn absolute_location_replaces_host() {
        // the relay -> PDS case: an absolute Location swaps the host entirely
        let current: http::Uri =
            "https://relay.example/xrpc/com.atproto.sync.getRepo?did=did:plc:hdhoaan3xa3jiuq4fg4mefid"
                .parse()
                .unwrap();
        let next = RedirectingClient::resolve_redirect(
            &current,
            "https://pds.example/xrpc/com.atproto.sync.getRepo?did=did:plc:hdhoaan3xa3jiuq4fg4mefid",
        )
        .expect("resolves");
        assert_eq!(next.scheme_str(), Some("https"));
        assert_eq!(next.host(), Some("pds.example"));
        assert_eq!(next.path(), "/xrpc/com.atproto.sync.getRepo");
    }

    #[test]
    fn relative_location_keeps_host_and_carries_query() {
        // a relative Location resolves against the current host, keeping it
        let current: http::Uri = "https://relay.example/xrpc/getRepo?did=x".parse().unwrap();
        let next = RedirectingClient::resolve_redirect(
            &current,
            "/xrpc/com.atproto.sync.getRepo?did=did:plc:hdhoaan3xa3jiuq4fg4mefid",
        )
        .expect("resolves");
        assert_eq!(next.scheme_str(), Some("https"));
        assert_eq!(next.host(), Some("relay.example"));
        assert_eq!(next.path(), "/xrpc/com.atproto.sync.getRepo");
        assert_eq!(next.query(), Some("did=did:plc:hdhoaan3xa3jiuq4fg4mefid"));
    }

    #[test]
    fn same_origin_ignores_default_port_path_and_query() {
        let a: http::Uri = "https://pds.example.com/xrpc/a".parse().unwrap();
        let b: http::Uri = "https://pds.example.com:443/xrpc/b?c=d".parse().unwrap();
        assert!(RedirectingClient::same_origin(&a, &b));
    }

    #[test]
    fn other_host_port_or_scheme_is_cross_origin() {
        // the Authorization-stripping trigger: a redirect anywhere but the
        // exact same origin must not carry auth headers along
        let base: http::Uri = "https://pds.example.com/x".parse().unwrap();
        for other in [
            "https://bad-example.com/x",
            "https://sub.pds.example.com/x",
            "https://pds.example.com:8443/x",
            "http://pds.example.com/x", // scheme downgrade changes default port
        ] {
            let other_uri: http::Uri = other.parse().unwrap();
            assert!(
                !RedirectingClient::same_origin(&base, &other_uri),
                "{other} should be cross-origin",
            );
        }
    }

    #[test]
    fn resolve_does_not_reject_scheme_downgrade() {
        // resolve_redirect is pure URL resolution — it does NOT enforce https.
        // the reqwest client's `https_only` rejects an http hop at send time.
        // documents where that check lives; resolve_redirect is the natural
        // home for an earlier https-only / SSRF guard if we want one.
        let current: http::Uri = "https://relay.example/x".parse().unwrap();
        let next = RedirectingClient::resolve_redirect(&current, "http://pds.example/y")
            .expect("resolves");
        assert_eq!(next.scheme_str(), Some("http"));
        assert_eq!(next.host(), Some("pds.example"));
    }
}
