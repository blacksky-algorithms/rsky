//! The managing-app access check (spec §managedspace).
//!
//! Under the `managedspace` policy the authority does NOT read the access
//! decision itself: it asks the space's `managingApp`, authenticated with a
//! service-auth JWT so the app can verify the call originates from the
//! space's authority. Reads and writes are checked separately.
//!
//! # Three methods, one question
//!
//! The method has been named three ways, and managing apps in the wild answer
//! each, so the authority tries them in [`CheckMethod::CHAIN`] order:
//!
//! 1. `com.atproto.managedspace.checkUserAccess` -- the 2026-10-29 spaces
//!    alpha this server implements: `{space, user, access, clientId}`,
//!    answering `{authorized}`.
//! 2. `com.atproto.managedspace.checkAccess` -- the reference since
//!    2026-11-04: `{space, repo, clientId}`, answering both `{read, write}`.
//! 3. `com.atproto.simplespace.checkUserAccess` -- the API before the
//!    alphas, which Acorn's feed service still serves: `{space, did,
//!    clientId}`, answering `{allowed}`. It has no access kind, so one answer
//!    stands for reads and writes alike, as it did under that API.
//!
//! It moves on only when a method is **not served** -- `404` or `501` without
//! an XRPC error naming anything but `MethodNotImplemented` -- never on an
//! answer, a refusal, or any other error, so a "no" can never become a "yes"
//! by asking elsewhere. Each call is signed for its own method (`lxm`). The
//! method an app answered is remembered per endpoint for the life of the
//! process, so an app on a later method pays the extra round trips once.

use async_trait::async_trait;
use rsky_lexicon::com::atproto::managedspace::{Access, CheckUserAccessOutput};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::error::{HostError, Result};
use crate::keys::{service_endpoint_from_doc, DocSource};
use crate::service_jwt;
use crate::signing::Signer;

pub const CHECK_USER_ACCESS_LXM: &str = "com.atproto.managedspace.checkUserAccess";
pub const CHECK_ACCESS_LXM: &str = "com.atproto.managedspace.checkAccess";
pub const LEGACY_CHECK_USER_ACCESS_LXM: &str = "com.atproto.simplespace.checkUserAccess";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// A method a managing app may answer the access check on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckMethod {
    /// `com.atproto.managedspace.checkUserAccess`, the 2026-10-29 alpha.
    CheckUserAccess,
    /// `com.atproto.managedspace.checkAccess`, the reference since 2026-11-04.
    CheckAccess,
    /// `com.atproto.simplespace.checkUserAccess`, the API before the alphas.
    LegacySimplespace,
}

impl CheckMethod {
    /// The order the methods are tried in: the one this server implements,
    /// then the newer, then the older.
    pub const CHAIN: [CheckMethod; 3] = [
        CheckMethod::CheckUserAccess,
        CheckMethod::CheckAccess,
        CheckMethod::LegacySimplespace,
    ];

    /// The method's NSID, also the `lxm` its service auth carries.
    pub fn nsid(self) -> &'static str {
        match self {
            CheckMethod::CheckUserAccess => CHECK_USER_ACCESS_LXM,
            CheckMethod::CheckAccess => CHECK_ACCESS_LXM,
            CheckMethod::LegacySimplespace => LEGACY_CHECK_USER_ACCESS_LXM,
        }
    }
}

/// `com.atproto.managedspace.checkAccess`'s answer.
#[derive(Deserialize)]
struct CheckAccessOutput {
    read: Option<bool>,
    write: Option<bool>,
}

/// The method each managing-app endpoint last answered on.
fn answered_by() -> &'static Mutex<HashMap<String, CheckMethod>> {
    static ANSWERED_BY: OnceLock<Mutex<HashMap<String, CheckMethod>>> = OnceLock::new();
    ANSWERED_BY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Whether a response says the method is not served at all, as opposed to an
/// answer or an error from a method that is. `501` is XRPC's
/// `MethodNotImplemented`; a `404` is what a server with no such route
/// answers, unless its body is an XRPC error naming something else, which
/// only a served method would send.
fn not_served(status: reqwest::StatusCode, body: &[u8]) -> bool {
    if status != reqwest::StatusCode::NOT_FOUND && status != reqwest::StatusCode::NOT_IMPLEMENTED {
        return false;
    }
    #[derive(Deserialize)]
    struct XrpcError {
        error: Option<String>,
    }
    match serde_json::from_slice::<XrpcError>(body)
        .ok()
        .and_then(|e| e.error)
    {
        Some(error) => error == "MethodNotImplemented",
        None => true,
    }
}

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
/// its DID document and asks it with authority service auth, on the first
/// method of [`CheckMethod::CHAIN`] it serves.
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

    /// Ask `endpoint` on one method. `Ok(None)` when the method is not
    /// served there; otherwise the decision, or the error the served method
    /// answered with.
    async fn ask(
        &self,
        endpoint: &str,
        method: CheckMethod,
        space: &str,
        user_did: &str,
        access: Access,
        client_id: Option<&str>,
    ) -> Result<Option<bool>> {
        let nsid = method.nsid();
        let token = service_jwt::mint(
            &self.signer,
            &self.authority_did,
            &self.service_id,
            nsid,
            (self.now)(),
            (self.jti)(),
        )?;
        let access_str = match access {
            Access::Read => "read",
            Access::Write => "write",
        };
        let mut query: Vec<(&str, &str)> = match method {
            // `user` is the alpha's name for the user. `did`, the name rsky
            // sent before, rides beside it for managing apps built to that.
            CheckMethod::CheckUserAccess => vec![
                ("space", space),
                ("user", user_did),
                ("did", user_did),
                ("access", access_str),
            ],
            // One call answers both kinds of access.
            CheckMethod::CheckAccess => vec![("space", space), ("repo", user_did)],
            CheckMethod::LegacySimplespace => vec![("space", space), ("did", user_did)],
        };
        if let Some(client_id) = client_id {
            query.push(("clientId", client_id));
        }
        let url = format!("{}/xrpc/{nsid}", endpoint.trim_end_matches('/'));
        let response = self
            .http
            .get(url)
            .query(&query)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| HostError::ManagingApp(e.to_string()))?;
        let status = response.status();
        let body = response
            .bytes()
            .await
            .map_err(|e| HostError::ManagingApp(e.to_string()))?;
        if not_served(status, &body) {
            return Ok(None);
        }
        // A served method's error -- a 403 included, which is how the legacy
        // method says the caller or client is not approved -- is no decision:
        // an error, so the caller can tell it from a refusal and retry.
        if !status.is_success() {
            return Err(HostError::ManagingApp(format!("{nsid} returned {status}")));
        }
        let decision = match method {
            CheckMethod::CheckUserAccess | CheckMethod::LegacySimplespace => {
                let output: CheckUserAccessOutput = serde_json::from_slice(&body)
                    .map_err(|e| HostError::ManagingApp(e.to_string()))?;
                if output.decision() == Some(false) {
                    tracing::debug!(space, user_did, reason = ?output.reason, "managing app denied access");
                }
                output.decision()
            }
            CheckMethod::CheckAccess => {
                let output: CheckAccessOutput = serde_json::from_slice(&body)
                    .map_err(|e| HostError::ManagingApp(e.to_string()))?;
                match access {
                    Access::Read => output.read,
                    Access::Write => output.write,
                }
            }
        };
        // An answer naming no decision is no answer.
        decision
            .map(Some)
            .ok_or_else(|| HostError::ManagingApp(format!("{nsid} answered no decision")))
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
        // The method this endpoint answered on before goes first; the rest
        // follow in chain order, so an app that changes method is followed.
        let remembered = answered_by()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&endpoint)
            .copied();
        let order = remembered.into_iter().chain(
            CheckMethod::CHAIN
                .into_iter()
                .filter(|m| Some(*m) != remembered),
        );
        for method in order {
            match self
                .ask(&endpoint, method, space, user_did, access, client_id)
                .await
            {
                Ok(None) => {
                    tracing::debug!(%endpoint, method = method.nsid(), "managing app does not serve this method");
                }
                answered => {
                    answered_by()
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .insert(endpoint.clone(), method);
                    return answered.map(|decision| decision.unwrap_or(false));
                }
            }
        }
        Err(HostError::ManagingApp(format!(
            "{endpoint} serves no access-check method"
        )))
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

    /// The `lxm` of each request a mock received, in order.
    async fn lxms(server: &MockServer) -> Vec<String> {
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|r| {
                let auth = r.headers.get("authorization").unwrap().to_str().unwrap();
                let jwt = auth.strip_prefix("Bearer ").unwrap();
                let payload = URL_SAFE_NO_PAD
                    .decode(jwt.split('.').nth(1).unwrap())
                    .unwrap();
                let payload: serde_json::Value = serde_json::from_slice(&payload).unwrap();
                payload["lxm"].as_str().unwrap().to_string()
            })
            .collect()
    }

    #[test]
    fn not_served_is_a_missing_route_not_an_answer() {
        use reqwest::StatusCode;
        assert!(not_served(StatusCode::NOT_FOUND, b"<html>404</html>"));
        assert!(not_served(StatusCode::NOT_FOUND, b""));
        assert!(not_served(
            StatusCode::NOT_IMPLEMENTED,
            br#"{"error":"MethodNotImplemented"}"#
        ));
        assert!(not_served(
            StatusCode::NOT_FOUND,
            br#"{"error":"MethodNotImplemented"}"#
        ));
        // A served method's own error is an answer, whatever its status.
        assert!(!not_served(
            StatusCode::NOT_FOUND,
            br#"{"error":"SpaceNotFound"}"#
        ));
        assert!(!not_served(StatusCode::FORBIDDEN, b""));
        assert!(!not_served(StatusCode::BAD_REQUEST, b""));
        assert!(!not_served(StatusCode::INTERNAL_SERVER_ERROR, b""));
    }

    /// An app on the reference's newer `checkAccess`: the alpha method is not
    /// implemented, so the authority asks `checkAccess` with `repo`, signed
    /// for it, and reads the access kind it wants from `{read, write}`. The
    /// second check goes straight there.
    #[tokio::test]
    async fn falls_back_to_check_access_and_remembers_it() {
        let server = MockServer::start().await;
        Mock::given(path(format!("/xrpc/{CHECK_USER_ACCESS_LXM}")))
            .respond_with(
                ResponseTemplate::new(501)
                    .set_body_json(serde_json::json!({"error": "MethodNotImplemented"})),
            )
            .mount(&server)
            .await;
        Mock::given(path(format!("/xrpc/{CHECK_ACCESS_LXM}")))
            .and(query_param("space", SPACE))
            .and(query_param("repo", "did:plc:member"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"read": true, "write": false})),
            )
            .mount(&server)
            .await;
        let app = client(&server.uri(), DEFAULT_TIMEOUT);
        let read = app
            .check_user_access(SPACE, "did:plc:member", Access::Read, None)
            .await
            .unwrap();
        let write = app
            .check_user_access(SPACE, "did:plc:member", Access::Write, None)
            .await
            .unwrap();
        assert!(read);
        assert!(!write);
        assert_eq!(
            lxms(&server).await,
            [CHECK_USER_ACCESS_LXM, CHECK_ACCESS_LXM, CHECK_ACCESS_LXM]
        );
    }

    /// An app on the legacy simplespace method, as Acorn's feed service is:
    /// a server with no route for either managedspace method answers a bare
    /// 404, and the legacy method's `{allowed}` decides reads and writes.
    #[tokio::test]
    async fn falls_back_to_the_legacy_simplespace_method() {
        let server = MockServer::start().await;
        Mock::given(path(format!("/xrpc/{LEGACY_CHECK_USER_ACCESS_LXM}")))
            .and(query_param("space", SPACE))
            .and(query_param("did", "did:plc:member"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"allowed": true})),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404).set_body_string("<html>Not Found</html>"))
            .mount(&server)
            .await;
        let app = client(&server.uri(), DEFAULT_TIMEOUT);
        assert!(app
            .check_user_access(
                SPACE,
                "did:plc:member",
                Access::Read,
                Some("https://app.example.com/client")
            )
            .await
            .unwrap());
        assert_eq!(
            lxms(&server).await,
            [
                CHECK_USER_ACCESS_LXM,
                CHECK_ACCESS_LXM,
                LEGACY_CHECK_USER_ACCESS_LXM
            ]
        );
        // The attested client reaches the legacy method too.
        let requests = server.received_requests().await.unwrap();
        assert!(requests[2].url.query().unwrap().contains("clientId="));
        // The same answer stands for a write, asked straight on the legacy
        // method this time.
        assert!(app
            .check_user_access(SPACE, "did:plc:member", Access::Write, None)
            .await
            .unwrap());
        assert_eq!(lxms(&server).await[3..], [LEGACY_CHECK_USER_ACCESS_LXM]);
    }

    /// A served method's refusal or error is final: nothing else is asked, so
    /// a "no" never becomes a "yes" elsewhere.
    #[tokio::test]
    async fn an_answer_or_an_error_never_falls_back() {
        for response in [
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"authorized": false})),
            ResponseTemplate::new(404).set_body_json(serde_json::json!({"error": "SpaceNotFound"})),
            ResponseTemplate::new(403).set_body_json(serde_json::json!({"error": "Forbidden"})),
        ] {
            let server = MockServer::start().await;
            Mock::given(path(format!("/xrpc/{CHECK_USER_ACCESS_LXM}")))
                .respond_with(response)
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({"read": true, "write": true, "allowed": true}),
                ))
                .mount(&server)
                .await;
            let app = client(&server.uri(), DEFAULT_TIMEOUT);
            let res = app
                .check_user_access(SPACE, "did:plc:member", Access::Read, None)
                .await;
            assert!(!matches!(res, Ok(true)), "{res:?}");
            assert_eq!(lxms(&server).await, [CHECK_USER_ACCESS_LXM]);
        }
    }

    #[tokio::test]
    async fn an_app_serving_no_method_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let app = client(&server.uri(), DEFAULT_TIMEOUT);
        let res = app
            .check_user_access(SPACE, "did:plc:member", Access::Read, None)
            .await;
        assert!(
            matches!(res, Err(HostError::ManagingApp(msg)) if msg.contains("serves no access-check method"))
        );
        assert_eq!(lxms(&server).await.len(), 3);
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
