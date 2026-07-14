//! HTTP client for `com.atproto.space.*` methods served by the space host
//! (authority): credential minting, the writer set, and notify registration.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rsky_lexicon::com::atproto::space::{
    GetSpaceCredentialInput, GetSpaceCredentialOutput, ListReposOutput, RegisterNotifyInput,
    RegisterNotifyOutput,
};
use serde::Deserialize;

use crate::error::{DaemonError, Result};

pub const XRPC_TIMEOUT_SECS: u64 = 30;

pub(crate) fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(XRPC_TIMEOUT_SECS))
        .build()
        .expect("static reqwest client configuration")
}

pub(crate) fn net_err(e: reqwest::Error) -> DaemonError {
    DaemonError::Xrpc(e.to_string())
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
    let body = resp.text().await.unwrap_or_default();
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
    ) -> Result<DateTime<Utc>>;
}

pub struct HttpSpaceHost {
    base_url: String,
    http: reqwest::Client,
}

impl HttpSpaceHost {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http: http_client(),
        }
    }

    fn url(&self, nsid: &str) -> String {
        format!("{}/xrpc/{nsid}", self.base_url)
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
            delegation_token: delegation_token.to_string(),
            client_attestation: client_attestation.map(str::to_string),
        };
        let resp = self
            .http
            .post(self.url("com.atproto.space.getSpaceCredential"))
            .json(&input)
            .send()
            .await
            .map_err(net_err)?;
        let out: GetSpaceCredentialOutput = check(resp).await?.json().await.map_err(net_err)?;
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
        let resp = self
            .http
            .get(self.url("com.atproto.space.listRepos"))
            .bearer_auth(credential)
            .query(&query)
            .send()
            .await
            .map_err(net_err)?;
        check(resp).await?.json().await.map_err(net_err)
    }

    async fn register_notify(
        &self,
        space: &str,
        credential: &str,
        endpoint: &str,
    ) -> Result<DateTime<Utc>> {
        let input = RegisterNotifyInput {
            space: space.to_string(),
            endpoint: endpoint.to_string(),
            repo: None,
        };
        let resp = self
            .http
            .post(self.url("com.atproto.space.registerNotify"))
            .bearer_auth(credential)
            .json(&input)
            .send()
            .await
            .map_err(net_err)?;
        let out: RegisterNotifyOutput = check(resp).await?.json().await.map_err(net_err)?;
        Ok(out.expires_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json_string, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SPACE: &str = "at://did:plc:authority/space/community.blacksky.feed/main";

    #[tokio::test]
    async fn get_space_credential_posts_input_and_returns_jwt() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.space.getSpaceCredential"))
            .and(body_json_string(format!(
                r#"{{"space":"{SPACE}","delegationToken":"dt.jwt","clientAttestation":"ca.jwt"}}"#
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "credential": "sc.jwt"
            })))
            .mount(&server)
            .await;

        let host = HttpSpaceHost::new(format!("{}/", server.uri()));
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
            .and(header("authorization", "Bearer sc.jwt"))
            .and(query_param("cursor", "c1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "repos": [{"did": "did:plc:b", "rev": "3kb"}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.space.listRepos"))
            .and(query_param("space", SPACE))
            .and(header("authorization", "Bearer sc.jwt"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "cursor": "c1",
                "repos": [{"did": "did:plc:a", "rev": "3ka", "hash": "ab12"}]
            })))
            .mount(&server)
            .await;

        let host = HttpSpaceHost::new(server.uri());
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
            .and(header("authorization", "Bearer sc.jwt"))
            .and(body_json_string(format!(
                r#"{{"space":"{SPACE}","endpoint":"https://syncer.example"}}"#
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "expiresAt": "2030-01-01T00:00:00Z"
            })))
            .mount(&server)
            .await;

        let host = HttpSpaceHost::new(server.uri());
        let expiry = host
            .register_notify(SPACE, "sc.jwt", "https://syncer.example")
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

        let host = HttpSpaceHost::new(server.uri());
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

        let host = HttpSpaceHost::new(server.uri());
        let err = host
            .get_space_credential(SPACE, "dt.jwt", None)
            .await
            .unwrap_err();
        assert!(matches!(err, DaemonError::Xrpc(m) if m.contains("bad gateway")));
    }

    #[tokio::test]
    async fn connection_failure_maps_to_xrpc_variant() {
        let host = HttpSpaceHost::new("http://127.0.0.1:1");
        let err = host
            .register_notify(SPACE, "sc.jwt", "https://syncer.example")
            .await
            .unwrap_err();
        assert!(matches!(err, DaemonError::Xrpc(_)));
    }
}
