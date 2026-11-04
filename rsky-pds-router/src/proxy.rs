//! Forwarding a request to one upstream, streaming both bodies, with the
//! class deadline and the distinction between a connection that failed
//! before anything was sent (safe to retry elsewhere) and everything else.

use bytes::Bytes;
use http::{HeaderMap, HeaderName, Method, Request, Response, StatusCode, Uri};
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt, Full};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;
pub type Body = UnsyncBoxBody<Bytes, BoxError>;

/// Headers that belong to one hop and are never forwarded.
const HOP_HEADERS: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    /// The upstream could not be reached; nothing was sent.
    #[error("connection to {0} failed: {1}")]
    Unreachable(String, String),
    /// The request was sent and then failed or timed out.
    #[error("{0} did not answer in time")]
    Timeout(String),
    #[error("{0}: {1}")]
    Upstream(String, String),
}

#[derive(Clone)]
pub struct Forwarder {
    client: Client<HttpConnector, Body>,
}

impl Default for Forwarder {
    fn default() -> Self {
        Self::new()
    }
}

impl Forwarder {
    pub fn new() -> Self {
        let mut connector = HttpConnector::new();
        connector.set_connect_timeout(Some(Duration::from_secs(3)));
        connector.set_nodelay(true);
        let client = Client::builder(TokioExecutor::new())
            .pool_idle_timeout(Duration::from_secs(60))
            .build(connector);
        Self { client }
    }

    /// Sends `method path?query` with `headers` and `body` to `upstream`
    /// (a scheme and authority), waiting at most `deadline` for the
    /// response headers.
    pub fn forward<'a>(
        &'a self,
        upstream: &'a str,
        method: Method,
        path_and_query: &'a str,
        headers: &'a HeaderMap,
        body: Body,
        deadline: Duration,
    ) -> Pin<Box<dyn Future<Output = Result<Response<Body>, ProxyError>> + Send + 'a>> {
        Box::pin(self.forward_inner(upstream, method, path_and_query, headers, body, deadline))
    }

    async fn forward_inner(
        &self,
        upstream: &str,
        method: Method,
        path_and_query: &str,
        headers: &HeaderMap,
        body: Body,
        deadline: Duration,
    ) -> Result<Response<Body>, ProxyError> {
        let uri: Uri = format!("{}{}", upstream.trim_end_matches('/'), path_and_query)
            .parse()
            .map_err(|e: http::uri::InvalidUri| {
                ProxyError::Upstream(upstream.to_owned(), e.to_string())
            })?;
        let mut request = Request::builder().method(method).uri(&uri);
        for (name, value) in headers {
            if HOP_HEADERS.contains(&name.as_str()) {
                continue;
            }
            request = request.header(name, value);
        }
        // the client's Host names the site the upstream serves; only a
        // request without one gets the upstream's authority
        if !headers.contains_key(http::header::HOST) {
            if let Some(authority) = uri.authority() {
                request = request.header(http::header::HOST, authority.as_str());
            }
        }
        let request = request
            .body(body)
            .map_err(|e| ProxyError::Upstream(upstream.to_owned(), e.to_string()))?;
        let sent = tokio::time::timeout(deadline, self.client.request(request)).await;
        match sent {
            Err(_) => Err(ProxyError::Timeout(upstream.to_owned())),
            Ok(Err(err)) if err.is_connect() => Err(ProxyError::Unreachable(
                upstream.to_owned(),
                err.to_string(),
            )),
            Ok(Err(err)) => Err(ProxyError::Upstream(upstream.to_owned(), err.to_string())),
            Ok(Ok(response)) => {
                let (mut parts, body) = response.into_parts();
                for name in HOP_HEADERS {
                    parts.headers.remove(*name);
                }
                Ok(Response::from_parts(
                    parts,
                    body.map_err(BoxError::from).boxed_unsync(),
                ))
            }
        }
    }
}

/// A body made of one buffer.
pub fn full_body(bytes: Bytes) -> Body {
    Full::new(bytes)
        .map_err(|never| match never {})
        .boxed_unsync()
}

/// An empty body.
pub fn empty_body() -> Body {
    full_body(Bytes::new())
}

/// A JSON error response in the XRPC shape.
pub fn json_error(status: StatusCode, error: &str, message: &str) -> Response<Body> {
    json_response(
        status,
        &serde_json::json!({ "error": error, "message": message }),
    )
}

/// A JSON document as a response.
pub fn json_response(status: StatusCode, value: &serde_json::Value) -> Response<Body> {
    let mut response = Response::new(full_body(Bytes::from(value.to_string())));
    *response.status_mut() = status;
    response.headers_mut().insert(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static("application/json"),
    );
    response
}

/// Names the backend and reason on a response.
pub fn stamp(response: &mut Response<Body>, backend: &str, reason: &str) {
    if let Ok(value) = http::HeaderValue::from_str(backend) {
        response
            .headers_mut()
            .insert(HeaderName::from_static("x-pds-backend"), value);
    }
    if let Ok(value) = http::HeaderValue::from_str(reason) {
        response
            .headers_mut()
            .insert(HeaderName::from_static("x-router-reason"), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_upstream_that_is_not_a_uri_is_an_upstream_error() {
        let forwarder = Forwarder::default();
        let headers = HeaderMap::new();
        let err = forwarder
            .forward(
                "http://bad host",
                Method::GET,
                "/xrpc/_health",
                &headers,
                empty_body(),
                Duration::from_secs(1),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ProxyError::Upstream(..)), "{err}");
    }
}
