use rsky_identity::safe_fetch::{NetworkPolicy, Redirects, SafeClient};
use rsky_oauth::client::ClientMetadataFetcher;
use rsky_oauth::jwk::JwkSet;
use rsky_oauth::types::OAuthClientMetadata;
use rsky_oauth::OAuthError;
use std::time::Duration;

const MAX_RESPONSE_SIZE: usize = 512 * 1024;
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// HTTPS fetcher for client metadata documents and JWK sets. Redirects are
/// never followed, only a 200 is accepted, and the body is read up to a
/// bound; the transport reaches only addresses the network policy permits.
pub struct HttpClientMetadataFetcher {
    client: SafeClient,
}

impl Default for HttpClientMetadataFetcher {
    fn default() -> Self {
        Self::new(crate::outbound::policy())
    }
}

impl HttpClientMetadataFetcher {
    pub fn new(policy: NetworkPolicy) -> Self {
        let client = SafeClient::new(policy, FETCH_TIMEOUT).expect("reqwest client");
        Self { client }
    }

    async fn fetch_json_capped(&self, url: &str) -> Result<Vec<u8>, OAuthError> {
        let invalid =
            |reason: String| OAuthError::InvalidClient(format!("failed to fetch {url}: {reason}"));
        let parsed = url::Url::parse(url).map_err(|e| invalid(e.to_string()))?;
        if parsed.scheme() != "https" {
            return Err(invalid("must be an https URL".to_string()));
        }
        self.client
            .check(&parsed)
            .map_err(|e| invalid(e.to_string()))?;
        let response = self
            .client
            .get(parsed, Redirects::None)
            .await
            .map_err(|e| invalid(e.to_string()))?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(invalid(format!("unexpected status {}", response.status())));
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if content_type != "application/json" {
            return Err(invalid(format!(
                "unexpected content-type \"{content_type}\""
            )));
        }
        let (_, body) = SafeClient::read_bounded(response, MAX_RESPONSE_SIZE)
            .await
            .map_err(|e| invalid(e.to_string()))?;
        Ok(body)
    }
}

#[async_trait::async_trait]
impl ClientMetadataFetcher for HttpClientMetadataFetcher {
    async fn fetch_client_metadata(&self, url: &str) -> Result<OAuthClientMetadata, OAuthError> {
        let body = self.fetch_json_capped(url).await?;
        serde_json::from_slice(&body).map_err(|e| {
            OAuthError::InvalidClient(format!("invalid client metadata document: {e}"))
        })
    }

    async fn fetch_jwks(&self, url: &str) -> Result<JwkSet, OAuthError> {
        let body = self.fetch_json_capped(url).await?;
        serde_json::from_slice(&body)
            .map_err(|e| OAuthError::InvalidClient(format!("invalid JWKS document: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejects_invalid_and_non_https_urls() {
        let fetcher = HttpClientMetadataFetcher::default();
        let err = fetcher
            .fetch_client_metadata("not a url")
            .await
            .unwrap_err();
        assert!(err.error_description().contains("failed to fetch"));
        let err = fetcher
            .fetch_client_metadata("http://app.example.com/client.json")
            .await
            .unwrap_err();
        assert!(err.error_description().contains("must be an https URL"));
        let err = fetcher
            .fetch_jwks("http://app.example.com/jwks.json")
            .await
            .unwrap_err();
        assert!(err.error_description().contains("must be an https URL"));
    }

    /// A one-shot HTTP/1.1 responder counting the connections it accepted.
    async fn responder(
        status: u16,
        headers: &'static str,
        body: &'static str,
    ) -> (u16, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let accepted = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = accepted.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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
        (port, accepted)
    }

    #[tokio::test]
    async fn metadata_is_never_fetched_through_a_redirect() {
        // https is required before any connection, so the redirect and its
        // target are reached over plain http only by a permissive policy
        // with the scheme check bypassed below through the transport
        let (target_port, target_hits) =
            responder(200, "Content-Type: application/json\r\n", "{}").await;
        let location: &'static str = Box::leak(
            format!("Location: http://127.0.0.1:{target_port}/client.json\r\n").into_boxed_str(),
        );
        let (redirect_port, redirect_hits) = responder(302, location, "").await;
        let fetcher = HttpClientMetadataFetcher::new(NetworkPolicy::PERMISSIVE);
        let response = fetcher
            .client
            .get(
                url::Url::parse(&format!("http://127.0.0.1:{redirect_port}/client.json")).unwrap(),
                Redirects::None,
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 302);
        assert_eq!(redirect_hits.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            target_hits.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "the redirect target was never requested"
        );
        // and the fetcher itself refuses the plain-http document outright
        let err = fetcher
            .fetch_client_metadata(&format!("http://127.0.0.1:{redirect_port}/client.json"))
            .await
            .unwrap_err();
        assert!(err.error_description().contains("must be an https URL"));
        assert_eq!(redirect_hits.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_public_policy_refuses_private_targets_before_connecting() {
        let (port, hits) = responder(200, "Content-Type: application/json\r\n", "{}").await;
        let fetcher = HttpClientMetadataFetcher::new(NetworkPolicy::PUBLIC);
        assert!(matches!(
            HttpClientMetadataFetcher::default().client.policy(),
            NetworkPolicy::PUBLIC | NetworkPolicy::PERMISSIVE
        ));
        let err = fetcher
            .fetch_jwks(&format!("https://127.0.0.1:{port}/jwks.json"))
            .await
            .unwrap_err();
        assert!(err.error_description().contains("loopback"), "{err:?}");
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn surfaces_connection_failures() {
        let fetcher = HttpClientMetadataFetcher::new(NetworkPolicy::PERMISSIVE);
        // nothing listens on port 1; the connection is refused immediately
        let err = fetcher
            .fetch_client_metadata("https://127.0.0.1:1/client.json")
            .await
            .unwrap_err();
        assert!(err.error_description().contains("failed to fetch"));
    }
}
