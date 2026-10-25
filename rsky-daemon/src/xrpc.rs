//! HTTP client for `com.atproto.space.*` methods served by the space host
//! (authority): credential minting, the writer set, and notify registration.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rsky_lexicon::com::atproto::space::{
    GetSpaceCredentialInput, GetSpaceCredentialOutput, ListReposOutput, RegisterNotifyInput,
    RegisterNotifyOutput,
};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::Arc;

use crate::dpop::DpopSigner;
use crate::error::{DaemonError, Result};
use crate::identity::PdsResolver;
use rsky_space::space_id::SpaceId;

pub const XRPC_TIMEOUT_SECS: u64 = 30;
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_CAR_RESPONSE_BYTES: usize = 256 * 1024 * 1024;

struct SystemResolver;

struct FilteringResolver {
    inner: Arc<dyn reqwest::dns::Resolve>,
}

impl FilteringResolver {
    fn system() -> Self {
        Self {
            inner: Arc::new(SystemResolver),
        }
    }

    #[cfg(test)]
    fn new(inner: Arc<dyn reqwest::dns::Resolve>) -> Self {
        Self { inner }
    }
}

impl reqwest::dns::Resolve for SystemResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map_err(|error| Box::new(error) as Box<dyn std::error::Error + Send + Sync>)?
                .collect();
            Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

impl reqwest::dns::Resolve for FilteringResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let resolving = self.inner.resolve(name);
        Box::pin(async move {
            let addresses = resolving.await?;
            let accepted: Vec<SocketAddr> = addresses
                .filter(|address| crate::identity::is_global_address(address.ip()))
                .collect();
            if accepted.is_empty() {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "DNS resolved only to non-global addresses",
                ))
                    as Box<dyn std::error::Error + Send + Sync>);
            }
            Ok(Box::new(accepted.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

pub(crate) fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(XRPC_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::none())
        .dns_resolver(Arc::new(FilteringResolver::system()))
        .build()
        .expect("static reqwest client configuration")
}

pub(crate) fn net_err(e: reqwest::Error) -> DaemonError {
    DaemonError::Xrpc(e.to_string())
}

pub(crate) async fn limited_bytes(response: reqwest::Response) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(DaemonError::Xrpc(
            "response body exceeds size limit".to_string(),
        ));
    }
    let bytes = response.bytes().await.map_err(net_err)?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(DaemonError::Xrpc(
            "response body exceeds size limit".to_string(),
        ));
    }
    Ok(bytes.to_vec())
}

pub(crate) async fn limited_json<T: DeserializeOwned>(response: reqwest::Response) -> Result<T> {
    let bytes = limited_bytes(response).await?;
    serde_json::from_slice(&bytes).map_err(|error| DaemonError::Xrpc(error.to_string()))
}

#[derive(Debug, Deserialize, Default)]
struct XrpcErrorBody {
    error: Option<String>,
    message: Option<String>,
}

/// Map a non-2xx XRPC response to a [`DaemonError`], surfacing the lexicon
/// `HistoryUnavailable` error as its own variant so callers can fall back to
/// full-state recovery.
pub(crate) async fn check(resp: reqwest::Response) -> Result<reqwest::Response> {
    if resp.status().is_success() {
        return Ok(resp);
    }
    let status = resp.status();
    let body = String::from_utf8_lossy(&limited_bytes(resp).await.unwrap_or_default()).into_owned();
    let parsed: XrpcErrorBody = serde_json::from_str(&body).unwrap_or_default();
    if parsed.error.as_deref() == Some("HistoryUnavailable") {
        return Err(DaemonError::HistoryUnavailable(
            parsed
                .message
                .unwrap_or_else(|| "since revision outside the host's oplog window".to_string()),
        ));
    }
    Err(DaemonError::Xrpc(format!("{status}: {body}")))
}

fn is_nonce_rejection(body: &XrpcErrorBody) -> bool {
    body.message
        .as_deref()
        .is_some_and(|m| m.contains("DPoP \"nonce\""))
}

fn is_nonce_challenge(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: &XrpcErrorBody,
) -> bool {
    body.error.as_deref() == Some("use_dpop_nonce")
        || (status == reqwest::StatusCode::UNAUTHORIZED
            && headers
                .get("WWW-Authenticate")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| {
                    value.to_ascii_lowercase().contains("dpop")
                        && value
                            .to_ascii_lowercase()
                            .contains("error=\"use_dpop_nonce\"")
                }))
}

pub(crate) async fn send_with_dpop_retry<F>(
    dpop: &DpopSigner,
    method: &str,
    url: &str,
    access_token: Option<&str>,
    build: F,
) -> Result<reqwest::Response>
where
    F: Fn(&str) -> Result<reqwest::RequestBuilder>,
{
    let mut retried = false;
    loop {
        let proof = dpop.proof(method, url, access_token)?;
        let response = build(&proof)?.send().await.map_err(net_err)?;
        if response.status().is_success() {
            return Ok(response);
        }
        let status = response.status();
        let response_headers = response.headers().clone();
        let nonce = response
            .headers()
            .get("DPoP-Nonce")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = response.text().await.unwrap_or_default();
        let parsed: XrpcErrorBody = serde_json::from_str(&body).unwrap_or_default();
        if !retried && is_nonce_challenge(status, &response_headers, &parsed) {
            if let Some(nonce) = nonce {
                dpop.set_nonce(url, nonce);
                retried = true;
                continue;
            }
        }
        // A host that rejects the nonce we hold without naming the error
        // `use_dpop_nonce` (or that issues no nonces at all) would otherwise
        // see that stale nonce on every request until the process restarts.
        if !retried && is_nonce_rejection(&parsed) {
            let parsed_url = reqwest::Url::parse(url).ok();
            let origin = parsed_url
                .as_ref()
                .and_then(|url| {
                    Some(format!(
                        "{}://{}:{}",
                        url.scheme(),
                        url.host_str()?,
                        url.port_or_known_default()?
                    ))
                })
                .unwrap_or_else(|| "invalid".to_string());
            let path = parsed_url
                .as_ref()
                .map(|url| url.path())
                .unwrap_or("<invalid-url>");
            tracing::warn!(
                origin = %origin,
                method,
                path,
                "rejected DPoP nonce; clearing cached nonce and retrying"
            );
            match nonce {
                Some(nonce) => dpop.set_nonce(url, nonce),
                None => dpop.clear_nonce(url),
            }
            retried = true;
            continue;
        }
        if parsed.error.as_deref() == Some("HistoryUnavailable") {
            return Err(DaemonError::HistoryUnavailable(
                parsed.message.unwrap_or_else(|| {
                    "since revision outside the host's oplog window".to_string()
                }),
            ));
        }
        return Err(DaemonError::Xrpc(format!("{status}: {body}")));
    }
}

/// Space-host methods the daemon consumes, abstracted for tests.
#[async_trait]
pub trait SpaceHostClient: Send + Sync {
    /// Exchange a delegation token (plus optional client attestation) for a
    /// space credential (`com.atproto.space.getSpaceCredential`).
    async fn get_space_credential(
        &self,
        space: &str,
        delegation_token: &str,
        client_attestation: Option<&str>,
    ) -> Result<String>;
    /// One page of the writer set (`com.atproto.space.listRepos`).
    async fn list_repos(
        &self,
        space: &str,
        credential: &str,
        cursor: Option<&str>,
        limit: Option<i64>,
    ) -> Result<ListReposOutput>;
    /// Register this syncer's notify endpoint; returns the registration expiry
    /// (`com.atproto.space.registerNotify`).
    async fn register_notify(
        &self,
        space: &str,
        credential: &str,
        endpoint: &str,
        service: Option<&str>,
    ) -> Result<DateTime<Utc>>;
}

pub struct HttpSpaceHost {
    base_url: String,
    http: reqwest::Client,
    dpop: Arc<DpopSigner>,
}

impl HttpSpaceHost {
    pub fn new(base_url: impl Into<String>, dpop: Arc<DpopSigner>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http: http_client(),
            dpop,
        }
    }

    fn url(&self, nsid: &str) -> String {
        format!("{}/xrpc/{nsid}", self.base_url)
    }

    pub async fn mint_internal_credential(
        &self,
        space: &str,
        service_jwt: &str,
        mint_token: &str,
    ) -> Result<String> {
        let url = format!("{}/admin/mintCredential", self.base_url);
        let response = check(
            self.http
                .post(&url)
                .header("Authorization", format!("Bearer {service_jwt}"))
                .header("X-Spacehost-Mint-Token", mint_token)
                .header("DPoP", self.dpop.proof("POST", &url, None)?)
                .query(&[("space", space)])
                .send()
                .await
                .map_err(net_err)?,
        )
        .await?;
        let out: GetSpaceCredentialOutput = limited_json(response).await?;
        Ok(out.credential)
    }
}

#[async_trait]
impl SpaceHostClient for HttpSpaceHost {
    async fn get_space_credential(
        &self,
        space: &str,
        delegation_token: &str,
        client_attestation: Option<&str>,
    ) -> Result<String> {
        let input = GetSpaceCredentialInput {
            space: space.to_string(),
            client_attestation: client_attestation.map(str::to_string),
        };
        // The delegation token is the request's bearer credential; the proof
        // beside it carries the key the minted credential binds to, and no
        // `ath`, because a grant is not a bound token.
        let url = self.url("com.atproto.space.getSpaceCredential");
        let out: GetSpaceCredentialOutput = limited_json(
            send_with_dpop_retry(&self.dpop, "POST", &url, None, |proof| {
                Ok(self
                    .http
                    .post(&url)
                    .bearer_auth(delegation_token)
                    .header("DPoP", proof)
                    .json(&input))
            })
            .await?,
        )
        .await?;
        Ok(out.credential)
    }

    async fn list_repos(
        &self,
        space: &str,
        credential: &str,
        cursor: Option<&str>,
        limit: Option<i64>,
    ) -> Result<ListReposOutput> {
        let mut query: Vec<(&str, String)> = vec![("space", space.to_string())];
        if let Some(limit) = limit {
            query.push(("limit", limit.to_string()));
        }
        if let Some(cursor) = cursor {
            query.push(("cursor", cursor.to_string()));
        }
        let url = self.url("com.atproto.space.listRepos");
        limited_json(
            send_with_dpop_retry(&self.dpop, "GET", &url, Some(credential), |proof| {
                Ok(self
                    .http
                    .get(&url)
                    .header("Authorization", format!("DPoP {credential}"))
                    .header("DPoP", proof)
                    .query(&query))
            })
            .await?,
        )
        .await
    }

    async fn register_notify(
        &self,
        space: &str,
        credential: &str,
        endpoint: &str,
        service: Option<&str>,
    ) -> Result<DateTime<Utc>> {
        let input = RegisterNotifyInput {
            space: space.to_string(),
            // Naming ourselves is what makes each delivery addressable to us
            // (proposals#100); the endpoint is sent either way, for a host
            // that cannot resolve the identifier.
            service: service.map(str::to_string),
            endpoint: Some(endpoint.to_string()),
            repo: None,
        };
        let url = self.url("com.atproto.space.registerNotify");
        let out: RegisterNotifyOutput = limited_json(
            send_with_dpop_retry(&self.dpop, "POST", &url, Some(credential), |proof| {
                Ok(self
                    .http
                    .post(&url)
                    .header("Authorization", format!("DPoP {credential}"))
                    .header("DPoP", proof)
                    .json(&input))
            })
            .await?,
        )
        .await?;
        Ok(out.expires_at)
    }
}

/// Sends each space's requests to the host named by its authority's DID
/// document, or to `fallback` when the document names none.
pub struct ResolvingSpaceHost {
    resolver: Arc<dyn PdsResolver>,
    fallback: String,
    http: reqwest::Client,
    dpop: Arc<DpopSigner>,
}

impl ResolvingSpaceHost {
    pub fn new(resolver: Arc<dyn PdsResolver>, fallback: &str, dpop: Arc<DpopSigner>) -> Self {
        Self {
            resolver,
            fallback: fallback.trim_end_matches('/').to_string(),
            http: http_client(),
            dpop,
        }
    }

    async fn host(&self, space: &str) -> Result<HttpSpaceHost> {
        let authority = SpaceId::parse(space)
            .map_err(|e| DaemonError::Xrpc(format!("invalid space {space}: {e}")))?
            .authority;
        let base_url = self
            .resolver
            .space_host_url(&authority)
            .await?
            .unwrap_or_else(|| self.fallback.clone());
        Ok(HttpSpaceHost {
            base_url,
            http: self.http.clone(),
            dpop: self.dpop.clone(),
        })
    }
}

#[async_trait]
impl SpaceHostClient for ResolvingSpaceHost {
    async fn get_space_credential(
        &self,
        space: &str,
        delegation_token: &str,
        client_attestation: Option<&str>,
    ) -> Result<String> {
        self.host(space)
            .await?
            .get_space_credential(space, delegation_token, client_attestation)
            .await
    }

    async fn list_repos(
        &self,
        space: &str,
        credential: &str,
        cursor: Option<&str>,
        limit: Option<i64>,
    ) -> Result<ListReposOutput> {
        self.host(space)
            .await?
            .list_repos(space, credential, cursor, limit)
            .await
    }

    async fn register_notify(
        &self,
        space: &str,
        credential: &str,
        endpoint: &str,
        service: Option<&str>,
    ) -> Result<DateTime<Utc>> {
        self.host(space)
            .await?
            .register_notify(space, credential, endpoint, service)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn test_dpop() -> Arc<DpopSigner> {
        Arc::new(DpopSigner::generate().unwrap())
    }
    use wiremock::matchers::{body_json_string, header, header_exists, method, path, query_param};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    const SPACE: &str = "at://did:plc:authority/space/community.blacksky.feed/main";

    struct NonceResponder(std::sync::atomic::AtomicUsize);

    impl Respond for NonceResponder {
        fn respond(&self, _request: &Request) -> ResponseTemplate {
            if self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                ResponseTemplate::new(400)
                    .insert_header("DPoP-Nonce", "nonce-1")
                    .set_body_json(serde_json::json!({"error": "use_dpop_nonce"}))
            } else {
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"repos": []}))
            }
        }
    }

    struct HeaderNonceResponder(std::sync::atomic::AtomicUsize);

    impl Respond for HeaderNonceResponder {
        fn respond(&self, _request: &Request) -> ResponseTemplate {
            if self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                ResponseTemplate::new(401)
                    .insert_header("WWW-Authenticate", r#"DPoP error="use_dpop_nonce""#)
                    .insert_header("DPoP-Nonce", "nonce-header")
            } else {
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"repos": []}))
            }
        }
    }

    struct StubResolver(Vec<SocketAddr>);

    impl reqwest::dns::Resolve for StubResolver {
        fn resolve(&self, _name: reqwest::dns::Name) -> reqwest::dns::Resolving {
            let addresses = self.0.clone();
            Box::pin(std::future::ready(Ok(
                Box::new(addresses.into_iter()) as reqwest::dns::Addrs
            )))
        }
    }

    #[tokio::test]
    async fn the_dns_filter_rejects_non_global_stub_addresses() {
        let resolver = FilteringResolver::new(Arc::new(StubResolver(vec!["127.0.0.1:443"
            .parse()
            .unwrap()])));
        let error = resolver
            .resolve(reqwest::dns::Name::from_str("pds.example").unwrap())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("non-global"));

        let resolver =
            FilteringResolver::new(Arc::new(StubResolver(vec!["8.8.8.8:443".parse().unwrap()])));
        assert!(resolver
            .resolve(reqwest::dns::Name::from_str("pds.example").unwrap())
            .await
            .unwrap()
            .next()
            .is_some());
    }

    #[tokio::test]
    async fn get_space_credential_posts_input_and_returns_jwt() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.space.getSpaceCredential"))
            .and(header("authorization", "Bearer dt.jwt"))
            .and(header_exists("dpop"))
            .and(body_json_string(format!(
                r#"{{"space":"{SPACE}","clientAttestation":"ca.jwt"}}"#
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "credential": "sc.jwt"
            })))
            .mount(&server)
            .await;

        let host = HttpSpaceHost::new(format!("{}/", server.uri()), test_dpop());
        let credential = host
            .get_space_credential(SPACE, "dt.jwt", Some("ca.jwt"))
            .await
            .unwrap();
        assert_eq!(credential, "sc.jwt");
    }

    #[tokio::test]
    async fn list_repos_paginates_with_cursor_and_bearer_credential() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.space.listRepos"))
            .and(query_param("space", SPACE))
            .and(query_param("limit", "2"))
            .and(header("authorization", "DPoP sc.jwt"))
            .and(header_exists("dpop"))
            .and(query_param("cursor", "c1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "repos": [{"did": "did:plc:b", "rev": "3kb"}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.space.listRepos"))
            .and(query_param("space", SPACE))
            .and(header("authorization", "DPoP sc.jwt"))
            .and(header_exists("dpop"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "cursor": "c1",
                "repos": [{"did": "did:plc:a", "rev": "3ka", "hash": "ab12"}]
            })))
            .mount(&server)
            .await;

        let host = HttpSpaceHost::new(server.uri(), test_dpop());
        let first = host.list_repos(SPACE, "sc.jwt", None, None).await.unwrap();
        assert_eq!(first.cursor.as_deref(), Some("c1"));
        assert_eq!(first.repos[0].did, "did:plc:a");
        assert_eq!(first.repos[0].hash.as_deref(), Some("ab12"));

        let second = host
            .list_repos(SPACE, "sc.jwt", Some("c1"), Some(2))
            .await
            .unwrap();
        assert_eq!(second.cursor, None);
        assert_eq!(second.repos[0].did, "did:plc:b");
    }

    #[tokio::test]
    async fn register_notify_returns_expiry() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.space.registerNotify"))
            .and(header("authorization", "DPoP sc.jwt"))
            .and(header_exists("dpop"))
            .and(body_json_string(format!(
                r#"{{"space":"{SPACE}","service":"did:web:syncer.example#atproto_space_syncer","endpoint":"https://syncer.example"}}"#
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "expiresAt": "2030-01-01T00:00:00Z"
            })))
            .mount(&server)
            .await;

        let host = HttpSpaceHost::new(server.uri(), test_dpop());
        let expiry = host
            .register_notify(
                SPACE,
                "sc.jwt",
                "https://syncer.example",
                Some("did:web:syncer.example#atproto_space_syncer"),
            )
            .await
            .unwrap();
        assert_eq!(expiry.to_rfc3339(), "2030-01-01T00:00:00+00:00");
    }

    #[tokio::test]
    async fn xrpc_error_body_maps_to_xrpc_variant() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.space.listRepos"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "InvalidRequest",
                "message": "bad space"
            })))
            .mount(&server)
            .await;

        let host = HttpSpaceHost::new(server.uri(), test_dpop());
        let err = host
            .list_repos(SPACE, "sc.jwt", None, None)
            .await
            .unwrap_err();
        assert!(matches!(err, DaemonError::Xrpc(m) if m.contains("InvalidRequest")));
    }

    #[tokio::test]
    async fn non_json_error_body_maps_to_xrpc_variant() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.space.getSpaceCredential"))
            .respond_with(ResponseTemplate::new(502).set_body_string("bad gateway"))
            .mount(&server)
            .await;

        let host = HttpSpaceHost::new(server.uri(), test_dpop());
        let err = host
            .get_space_credential(SPACE, "dt.jwt", None)
            .await
            .unwrap_err();
        assert!(matches!(err, DaemonError::Xrpc(m) if m.contains("bad gateway")));
    }

    #[tokio::test]
    async fn connection_failure_maps_to_xrpc_variant() {
        let host = HttpSpaceHost::new("http://127.0.0.1:1", test_dpop());
        let err = host
            .register_notify(
                SPACE,
                "sc.jwt",
                "https://syncer.example",
                Some("did:web:syncer.example#atproto_space_syncer"),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, DaemonError::Xrpc(_)));
    }

    #[tokio::test]
    async fn retries_once_after_server_nonce_challenge() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.space.listRepos"))
            .respond_with(NonceResponder(std::sync::atomic::AtomicUsize::new(0)))
            .mount(&server)
            .await;

        let host = HttpSpaceHost::new(server.uri(), test_dpop());
        assert!(host.list_repos(SPACE, "sc.jwt", None, None).await.is_ok());
    }

    #[tokio::test]
    async fn retries_after_rfc9449_header_nonce_challenge() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.space.listRepos"))
            .respond_with(HeaderNonceResponder(std::sync::atomic::AtomicUsize::new(0)))
            .mount(&server)
            .await;
        let host = HttpSpaceHost::new(server.uri(), test_dpop());
        assert!(host.list_repos(SPACE, "sc.jwt", None, None).await.is_ok());
        assert_eq!(server.received_requests().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn refuses_an_oversized_response_body() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.space.listRepos"))
            .respond_with(
                ResponseTemplate::new(200).set_body_bytes(vec![b'x'; MAX_RESPONSE_BYTES + 1]),
            )
            .mount(&server)
            .await;
        let host = HttpSpaceHost::new(server.uri(), test_dpop());
        let err = host
            .list_repos(SPACE, "sc.jwt", None, None)
            .await
            .unwrap_err();
        assert!(matches!(err, DaemonError::Xrpc(message) if message.contains("size limit")));
    }

    #[tokio::test]
    async fn a_nonce_challenge_is_retried_once_and_needs_a_nonce() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.space.listRepos"))
            .and(header_exists("dpop"))
            .respond_with(
                ResponseTemplate::new(400)
                    .insert_header("DPoP-Nonce", "nonce-1")
                    .set_body_json(serde_json::json!({"error": "use_dpop_nonce"})),
            )
            .mount(&server)
            .await;
        let host = HttpSpaceHost::new(server.uri(), test_dpop());
        let err = host
            .list_repos(SPACE, "sc.jwt", None, None)
            .await
            .unwrap_err();
        assert!(matches!(err, DaemonError::Xrpc(_)));
        assert_eq!(server.received_requests().await.unwrap().len(), 2);

        server.reset().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_json(serde_json::json!({"error": "use_dpop_nonce"})),
            )
            .mount(&server)
            .await;
        let err = host
            .list_repos(SPACE, "sc.jwt", None, None)
            .await
            .unwrap_err();
        assert!(matches!(err, DaemonError::Xrpc(_)));
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    fn proof_nonce(request: &Request) -> Option<String> {
        let proof = request.headers.get("dpop")?.to_str().ok()?;
        rsky_oauth::jwt::decode(proof)
            .ok()?
            .claims
            .extra
            .get("nonce")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
    }

    struct RejectsAnyNonce;

    impl Respond for RejectsAnyNonce {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            match proof_nonce(request) {
                Some(_) => ResponseTemplate::new(401).set_body_json(serde_json::json!({
                    "error": "AuthenticationRequired",
                    "message": "InvalidDpopProof: DPoP \"nonce\" mismatch"
                })),
                None => ResponseTemplate::new(200).set_body_json(serde_json::json!({"repos": []})),
            }
        }
    }

    #[tokio::test]
    async fn a_rejected_nonce_without_a_replacement_is_dropped() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.space.listRepos"))
            .respond_with(RejectsAnyNonce)
            .mount(&server)
            .await;
        let dpop = test_dpop();
        dpop.set_nonce(&server.uri(), "stale".to_string());
        let host = HttpSpaceHost::new(server.uri(), dpop);

        assert!(host.list_repos(SPACE, "sc.jwt", None, None).await.is_ok());
        assert!(host.list_repos(SPACE, "sc.jwt", None, None).await.is_ok());
        let requests = server.received_requests().await.unwrap();
        let nonces: Vec<_> = requests.iter().map(proof_nonce).collect();
        assert_eq!(nonces, vec![Some("stale".to_string()), None, None]);
    }

    #[tokio::test]
    async fn a_rejected_nonce_is_replaced_by_the_offered_one() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.space.listRepos"))
            .respond_with(|request: &Request| {
                if proof_nonce(request).as_deref() == Some("fresh") {
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({"repos": []}))
                } else {
                    ResponseTemplate::new(401)
                        .insert_header("DPoP-Nonce", "fresh")
                        .set_body_json(serde_json::json!({
                            "error": "AuthenticationRequired",
                            "message": "InvalidDpopProof: DPoP \"nonce\" mismatch"
                        }))
                }
            })
            .mount(&server)
            .await;
        let dpop = test_dpop();
        dpop.set_nonce(&server.uri(), "stale".to_string());
        let host = HttpSpaceHost::new(server.uri(), dpop);

        assert!(host.list_repos(SPACE, "sc.jwt", None, None).await.is_ok());
        assert_eq!(server.received_requests().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_persistent_nonce_rejection_is_retried_only_once() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.space.listRepos"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": "AuthenticationRequired",
                "message": "InvalidDpopProof: DPoP \"nonce\" mismatch"
            })))
            .mount(&server)
            .await;
        let host = HttpSpaceHost::new(server.uri(), test_dpop());

        let err = host
            .list_repos(SPACE, "sc.jwt", None, None)
            .await
            .unwrap_err();
        assert!(matches!(err, DaemonError::Xrpc(_)));
        assert_eq!(server.received_requests().await.unwrap().len(), 2);
    }
}

#[cfg(test)]
mod resolving_tests {
    use super::*;
    use crate::identity::tests::plc_serving;
    use crate::identity::{DidResolver, PDS_ENDPOINT_TTL};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn space_host_answering(credential: &str) -> MockServer {
        let host = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.space.getSpaceCredential"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"credential": credential})),
            )
            .mount(&host)
            .await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.space.listRepos"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"repos": []})),
            )
            .mount(&host)
            .await;
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.space.registerNotify"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"expiresAt": "2030-01-01T00:00:00Z"})),
            )
            .mount(&host)
            .await;
        host
    }

    async fn paths(server: &MockServer) -> Vec<String> {
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|r| r.url.path().to_string())
            .collect()
    }

    #[tokio::test]
    async fn each_space_goes_to_its_authoritys_pds_or_the_fallback() {
        let one = space_host_answering("from-one").await;
        let two = space_host_answering("from-two").await;
        let fallback = space_host_answering("from-fallback").await;
        let plc = plc_serving(&[
            ("did:plc:one", Some(&one.uri())),
            ("did:plc:two", Some(&format!("{}/", two.uri()))),
            ("did:plc:nopds", None),
        ])
        .await;
        let host = ResolvingSpaceHost::new(
            Arc::new(DidResolver::new(Some(plc.uri()), PDS_ENDPOINT_TTL, true)),
            &format!("{}/", fallback.uri()),
            Arc::new(DpopSigner::generate().unwrap()),
        );
        let space =
            |authority: &str| format!("at://{authority}/space/community.blacksky.feed/main");
        assert_eq!(
            host.get_space_credential(&space("did:plc:one"), "dt", None)
                .await
                .unwrap(),
            "from-one"
        );
        assert_eq!(
            host.get_space_credential(&space("did:plc:two"), "dt", None)
                .await
                .unwrap(),
            "from-two"
        );
        assert_eq!(
            host.get_space_credential(&space("did:plc:nopds"), "dt", None)
                .await
                .unwrap(),
            "from-fallback"
        );
        assert!(
            host.get_space_credential("not a space", "dt", None)
                .await
                .is_err()
        );
        assert!(
            host.get_space_credential(&space("did:plc:unresolvable"), "dt", None)
                .await
                .is_err()
        );

        host.list_repos(&space("did:plc:two"), "sc", None, None)
            .await
            .unwrap();
        host.register_notify(&space("did:plc:two"), "sc", "https://d/notify", None)
            .await
            .unwrap();
        assert_eq!(paths(&one).await.len(), 1);
        assert_eq!(
            paths(&two).await,
            vec![
                "/xrpc/com.atproto.space.getSpaceCredential",
                "/xrpc/com.atproto.space.listRepos",
                "/xrpc/com.atproto.space.registerNotify",
            ]
        );
        assert_eq!(
            paths(&fallback).await.len(),
            1,
            "an unresolvable authority is never sent to the fallback"
        );
    }
}
