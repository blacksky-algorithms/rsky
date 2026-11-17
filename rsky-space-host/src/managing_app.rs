//! The managing-app access check (spec §managedspace).
//!
//! Under the `managedspace` policy the authority does NOT read the access
//! decision itself: it asks the space's `managingApp` via
//! `com.atproto.managedspace.checkUserAccess`, authenticated with a
//! service-auth JWT so the app can verify the call originates from the
//! space's authority. Reads and writes are checked separately.

use async_trait::async_trait;
use rsky_lexicon::com::atproto::managedspace::{Access, CheckUserAccessOutput};
use std::sync::Arc;
use std::time::Duration;

use crate::error::{HostError, Result};
use crate::keys::{service_endpoint_from_doc, DocSource};
use crate::service_jwt;
use crate::signing::Signer;

pub const CHECK_USER_ACCESS_LXM: &str = "com.atproto.managedspace.checkUserAccess";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Asks the managing app whether a user may read or write a space.
#[async_trait]
pub trait ManagingAppClient: Send + Sync {
    async fn check_user_access(
        &self,
        space: &str,
        user_did: &str,
        access: Access,
        client_id: Option<&str>,
    ) -> Result<bool>;
}

/// Split a service identifier into its DID and, when it has one, its
/// fragment. The fragment is optional (`did:web:example.com` or
/// `did:web:example.com#forum`), as upstream `atproto-pds` reads it; an empty
/// one (`did:web:example.com#`) is not a fragment and is refused.
pub fn split_service_id(service_id: &str) -> Result<(&str, Option<&str>)> {
    let invalid = || HostError::ManagingApp(format!("invalid service identifier: {service_id}"));
    let (did, fragment) = match service_id.split_once('#') {
        Some((_, "")) => return Err(invalid()),
        Some((did, fragment)) => (did, Some(fragment)),
        None => (service_id, None),
    };
    if !did.starts_with("did:") || did.len() <= "did:".len() {
        return Err(invalid());
    }
    Ok((did, fragment))
}

/// Reject plain-http endpoints, except loopback hosts (local testing).
pub(crate) fn require_https(url: &str) -> Result<()> {
    if url.starts_with("https://") {
        return Ok(());
    }
    if let Some(rest) = url.strip_prefix("http://") {
        let host = rest.split(['/', ':']).next().unwrap_or("");
        if host == "localhost" || host == "127.0.0.1" {
            return Ok(());
        }
    }
    Err(HostError::ManagingApp(format!("endpoint not https: {url}")))
}

/// HTTP [`ManagingAppClient`]: resolves the managing app's service endpoint from
/// its DID document and calls `checkUserAccess` with authority service auth.
pub struct HttpManagingApp {
    service_id: String,
    authority_did: String,
    signer: Signer,
    docs: Arc<dyn DocSource>,
    http: reqwest::Client,
    now: Arc<dyn Fn() -> u64 + Send + Sync>,
    jti: Arc<dyn Fn() -> String + Send + Sync>,
}

impl HttpManagingApp {
    pub fn new(
        service_id: String,
        authority_did: String,
        signer: Signer,
        docs: Arc<dyn DocSource>,
        now: Arc<dyn Fn() -> u64 + Send + Sync>,
        jti: Arc<dyn Fn() -> String + Send + Sync>,
    ) -> Self {
        Self::with_timeout(
            service_id,
            authority_did,
            signer,
            docs,
            now,
            jti,
            DEFAULT_TIMEOUT,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_timeout(
        service_id: String,
        authority_did: String,
        signer: Signer,
        docs: Arc<dyn DocSource>,
        now: Arc<dyn Fn() -> u64 + Send + Sync>,
        jti: Arc<dyn Fn() -> String + Send + Sync>,
        timeout: Duration,
    ) -> Self {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("reqwest client");
        Self {
            service_id,
            authority_did,
            signer,
            docs,
            http,
            now,
            jti,
        }
    }

    async fn endpoint(&self) -> Result<String> {
        let (did, fragment) = split_service_id(&self.service_id)?;
        let doc = self.docs.did_document(did).await?;
        // With no fragment, the document's first service, as upstream
        // `atproto-pds` resolves one (`space::recipient`).
        let endpoint = match fragment {
            Some(fragment) => service_endpoint_from_doc(&doc, fragment)?,
            None => doc
                .service
                .as_deref()
                .and_then(|services| services.first())
                .map(|s| s.service_endpoint.clone())
                .ok_or_else(|| HostError::Resolution(format!("{}: no service", doc.id)))?,
        };
        require_https(&endpoint)?;
        Ok(endpoint)
    }
}

#[async_trait]
impl ManagingAppClient for HttpManagingApp {
    async fn check_user_access(
        &self,
        space: &str,
        user_did: &str,
        access: Access,
        client_id: Option<&str>,
    ) -> Result<bool> {
        let endpoint = self.endpoint().await?;
        let token = service_jwt::mint(
            &self.signer,
            &self.authority_did,
            &self.service_id,
            CHECK_USER_ACCESS_LXM,
            (self.now)(),
            (self.jti)(),
        )?;
        let access_str = match access {
            Access::Read => "read",
            Access::Write => "write",
        };
        // `user` is the 2026-10-29 alpha's name for the user. `did`, the name
        // rsky sent before, rides beside it for managing apps built to that.
        let mut query: Vec<(&str, &str)> = vec![
            ("space", space),
            ("user", user_did),
            ("did", user_did),
            ("access", access_str),
        ];
        if let Some(client_id) = client_id {
            query.push(("clientId", client_id));
        }
        let url = format!(
            "{}/xrpc/{CHECK_USER_ACCESS_LXM}",
            endpoint.trim_end_matches('/')
        );
        let response = self
            .http
            .get(url)
            .query(&query)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| HostError::ManagingApp(e.to_string()))?;
        if !response.status().is_success() {
            return Err(HostError::ManagingApp(format!(
                "checkUserAccess returned {}",
                response.status()
            )));
        }
        let output: CheckUserAccessOutput = response
            .json()
            .await
            .map_err(|e| HostError::ManagingApp(e.to_string()))?;
        // An answer naming no decision is no answer: an error, so the caller
        // can tell it from a refusal.
        let authorized = output.decision().ok_or_else(|| {
            HostError::ManagingApp("checkUserAccess answered no `authorized`".to_string())
        })?;
        if !authorized {
            tracing::debug!(space, user_did, reason = ?output.reason, "managing app denied access");
        }
        Ok(authorized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signing::test_signer;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use rsky_identity::types::{DidDocument, Service};
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SPACE: &str = "at://did:plc:auth/space/community.blacksky.feed/main";
    const APP_DID: &str = "did:web:app.example.com";

    struct AppDoc(String);
    #[async_trait]
    impl DocSource for AppDoc {
        async fn did_document(&self, did: &str) -> Result<DidDocument> {
            Ok(DidDocument {
                context: None,
                id: did.to_string(),
                also_known_as: None,
                verification_method: None,
                service: Some(vec![Service {
                    id: format!("{did}#managing_app"),
                    r#type: "ManagingApp".to_string(),
                    service_endpoint: self.0.clone(),
                }]),
            })
        }
    }

    fn client(endpoint: &str, timeout: Duration) -> HttpManagingApp {
        HttpManagingApp::with_timeout(
            format!("{APP_DID}#managing_app"),
            "did:plc:auth".to_string(),
            test_signer(),
            Arc::new(AppDoc(endpoint.to_string())),
            Arc::new(|| 1000),
            Arc::new(|| "jti-fixed".to_string()),
            timeout,
        )
    }

    #[tokio::test]
    async fn allow_path_sends_service_auth_jwt_with_expected_claims() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("/xrpc/{CHECK_USER_ACCESS_LXM}")))
            .and(query_param("space", SPACE))
            // The 2026-10-29 alpha's `user`, and the earlier `did` beside it.
            .and(query_param("user", "did:plc:member"))
            .and(query_param("did", "did:plc:member"))
            .and(query_param("access", "read"))
            .and(query_param("clientId", "https://app.example.com/client"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"authorized": true})),
            )
            .mount(&server)
            .await;

        // The plain constructor (default timeout) drives the happy path.
        let app = HttpManagingApp::new(
            format!("{APP_DID}#managing_app"),
            "did:plc:auth".to_string(),
            test_signer(),
            Arc::new(AppDoc(server.uri())),
            Arc::new(|| 1000),
            Arc::new(|| "jti-fixed".to_string()),
        );
        let allowed = app
            .check_user_access(
                SPACE,
                "did:plc:member",
                Access::Read,
                Some("https://app.example.com/client"),
            )
            .await
            .unwrap();
        assert!(allowed);

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let auth = requests[0].headers.get("authorization").unwrap();
        let jwt = auth.to_str().unwrap().strip_prefix("Bearer ").unwrap();
        let claims = crate::service_jwt::verify(
            jwt,
            &[&format!("{APP_DID}#managing_app")],
            CHECK_USER_ACCESS_LXM,
            test_signer().did_key(),
            1000,
        )
        .unwrap();
        assert_eq!(claims.iss, "did:plc:auth");
        assert_eq!(claims.aud, format!("{APP_DID}#managing_app"));
        assert_eq!(claims.lxm.as_deref(), Some(CHECK_USER_ACCESS_LXM));
        assert_eq!(claims.exp, 1060);
        // Payload jti matches the injected nonce.
        let payload = URL_SAFE_NO_PAD
            .decode(jwt.split('.').nth(1).unwrap())
            .unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(payload["jti"], "jti-fixed");
    }

    #[tokio::test]
    async fn deny_path_returns_false_without_client_id() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"allowed": false, "reason": "not a member"})),
            )
            .mount(&server)
            .await;
        let app = client(&server.uri(), DEFAULT_TIMEOUT);
        let allowed = app
            .check_user_access(SPACE, "did:plc:stranger", Access::Write, None)
            .await
            .unwrap();
        assert!(!allowed);
        // No clientId param when there is no attested client.
        let requests = server.received_requests().await.unwrap();
        assert!(!requests[0].url.query().unwrap().contains("clientId"));
    }

    #[tokio::test]
    async fn server_error_and_bad_body_are_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let app = client(&server.uri(), DEFAULT_TIMEOUT);
        let res = app
            .check_user_access(SPACE, "did:plc:member", Access::Read, None)
            .await;
        assert!(matches!(res, Err(HostError::ManagingApp(msg)) if msg.contains("500")));

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;
        let app = client(&server.uri(), DEFAULT_TIMEOUT);
        assert!(app
            .check_user_access(SPACE, "did:plc:member", Access::Read, None)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn timeout_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"allowed": true}))
                    .set_delay(Duration::from_secs(2)),
            )
            .mount(&server)
            .await;
        let app = client(&server.uri(), Duration::from_millis(100));
        let res = app
            .check_user_access(SPACE, "did:plc:member", Access::Read, None)
            .await;
        assert!(matches!(res, Err(HostError::ManagingApp(_))));
    }

    #[tokio::test]
    async fn invalid_service_ids_and_endpoints_rejected() {
        assert_eq!(
            split_service_id("did:plc:app").unwrap(),
            ("did:plc:app", None)
        );
        assert!(split_service_id("not-a-did#frag").is_err());
        assert!(split_service_id("not-a-did").is_err());
        assert!(split_service_id("did:").is_err());
        assert!(split_service_id("did:plc:app#").is_err());
        assert_eq!(
            split_service_id("did:plc:app#managing_app").unwrap(),
            ("did:plc:app", Some("managing_app"))
        );

        require_https("https://app.example.com").unwrap();
        require_https("http://localhost:3000").unwrap();
        require_https("http://127.0.0.1:3000/base").unwrap();
        assert!(require_https("http://app.example.com").is_err());
        assert!(require_https("ftp://app.example.com").is_err());
        assert!(require_https("http://").is_err());

        // A doc pointing at a non-https endpoint fails before any request.
        let app = client("http://app.example.com", DEFAULT_TIMEOUT);
        assert!(app
            .check_user_access(SPACE, "did:plc:member", Access::Read, None)
            .await
            .is_err());
    }
}
