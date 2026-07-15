use crate::APP_USER_AGENT;
use rsky_oauth::client::ClientMetadataFetcher;
use rsky_oauth::jwk::JwkSet;
use rsky_oauth::types::OAuthClientMetadata;
use rsky_oauth::OAuthError;
use std::time::Duration;

const MAX_RESPONSE_SIZE: usize = 512 * 1024;
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// HTTPS fetcher for client metadata documents and JWK sets.
pub struct HttpClientMetadataFetcher {
    client: reqwest::Client,
}

impl Default for HttpClientMetadataFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpClientMetadataFetcher {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent(APP_USER_AGENT)
            .timeout(FETCH_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("reqwest client construction cannot fail");
        Self { client }
    }

    async fn fetch_json_capped(&self, url: &str) -> Result<Vec<u8>, OAuthError> {
        let invalid =
            |reason: String| OAuthError::InvalidClient(format!("failed to fetch {url}: {reason}"));
        let parsed = url::Url::parse(url).map_err(|e| invalid(e.to_string()))?;
        if parsed.scheme() != "https" {
            return Err(invalid("must be an https URL".to_string()));
        }
        let response = self
            .client
            .get(parsed)
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|e| invalid(e.to_string()))?;
        if !response.status().is_success() {
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
        let body = response.bytes().await.map_err(|e| invalid(e.to_string()))?;
        if body.len() > MAX_RESPONSE_SIZE {
            return Err(invalid("response too large".to_string()));
        }
        Ok(body.to_vec())
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

    #[tokio::test]
    async fn surfaces_connection_failures() {
        let fetcher = HttpClientMetadataFetcher::new();
        // nothing listens on port 1; the connection is refused immediately
        let err = fetcher
            .fetch_client_metadata("https://127.0.0.1:1/client.json")
            .await
            .unwrap_err();
        assert!(err.error_description().contains("failed to fetch"));
    }
}
