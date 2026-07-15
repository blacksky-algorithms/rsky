//! The space host's XRPC surface (spec §XRPC API, host methods).
//!
//! Request/response shapes are the `com.atproto.space.*` DTOs from
//! rsky-lexicon; errors are XRPC-shaped `{error, message}` JSON.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rsky_lexicon::com::atproto::space::{
    GetSpaceCredentialInput, GetSpaceCredentialOutput, GetSpaceOutput, GetSpaceParams,
    ListReposOutput, ListReposParams, NotifyWriteInput, RegisterNotifyInput, RegisterNotifyOutput,
    SpaceConfig,
};
use rsky_space::credential;
use std::sync::Arc;

use crate::attestation::{JtiStore, MetadataFetcher};
use crate::authority::{Authority, KeyResolver};
use crate::error::HostError;
use crate::managing_app::require_https;
use crate::notify::{fan_out_write, Notifier, NOTIFY_WRITE_LXM};
use crate::policy::Policy;
use crate::service_jwt;
use crate::store::{RegistrationStore, WriterSetStore};

pub const DEFAULT_REGISTRATION_TTL_SECS: u64 = 24 * 60 * 60;
const DEFAULT_LIST_LIMIT: i64 = 100;
const MAX_LIST_LIMIT: i64 = 1000;

#[derive(Clone)]
pub struct AppState {
    pub authority: Arc<Authority>,
    pub policy: Arc<Policy>,
    pub keys: Arc<dyn KeyResolver>,
    pub metadata: Arc<dyn MetadataFetcher>,
    pub jti_store: Arc<dyn JtiStore>,
    pub writers: Arc<dyn WriterSetStore>,
    pub registrations: Arc<dyn RegistrationStore>,
    pub notifier: Arc<dyn Notifier>,
    pub now: Arc<dyn Fn() -> u64 + Send + Sync>,
    pub jti: Arc<dyn Fn() -> String + Send + Sync>,
    pub registration_ttl_secs: u64,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/xrpc/_health", get(health))
        .route("/xrpc/com.atproto.space.getSpace", get(get_space))
        .route(
            "/xrpc/com.atproto.space.getSpaceCredential",
            post(get_space_credential),
        )
        .route("/xrpc/com.atproto.space.listRepos", get(list_repos))
        .route(
            "/xrpc/com.atproto.space.registerNotify",
            post(register_notify),
        )
        .route("/xrpc/com.atproto.space.notifyWrite", post(notify_write))
        .with_state(state)
}

/// An XRPC-shaped error response: `{error, message}` with a matching status.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    error: &'static str,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, error: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            error,
            message: message.into(),
        }
    }

    fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "InvalidRequest", message)
    }

    fn auth_required(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "AuthenticationRequired", message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({"error": self.error, "message": self.message});
        (self.status, Json(body)).into_response()
    }
}

impl From<HostError> for ApiError {
    fn from(e: HostError) -> Self {
        match &e {
            HostError::Delegation(m) => {
                Self::new(StatusCode::UNAUTHORIZED, "InvalidToken", m.clone())
            }
            HostError::AttestationRequired => Self::new(
                StatusCode::UNAUTHORIZED,
                "AttestationRequired",
                "space requires a client attestation",
            ),
            HostError::Attestation(m) => {
                Self::new(StatusCode::UNAUTHORIZED, "InvalidAttestation", m.clone())
            }
            HostError::NotAuthorized => Self::new(
                StatusCode::FORBIDDEN,
                "NotAuthorized",
                "user not authorized for space",
            ),
            HostError::ClientNotAuthorized => Self::new(
                StatusCode::FORBIDDEN,
                "ClientNotAuthorized",
                "client not authorized for space",
            ),
            HostError::Key(_)
            | HostError::Membership(_)
            | HostError::ManagingApp(_)
            | HostError::Resolution(_)
            | HostError::Store(_)
            | HostError::Space(_) => {
                tracing::error!(error = %e, "internal error");
                Self::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "InternalError",
                    "internal error",
                )
            }
        }
    }
}

fn bearer(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| ApiError::auth_required("missing bearer token"))
}

/// Space-credential auth: verify the presented credential against this
/// authority's own space key.
fn require_space_credential(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let jwt = bearer(headers)?;
    credential::verify_space_credential(
        jwt,
        &state.authority.space_uri(),
        state.authority.authority_did(),
        state.authority.signer.did_key(),
        (state.now)(),
    )
    .map_err(|e| ApiError::new(StatusCode::UNAUTHORIZED, "InvalidToken", e.to_string()))
}

fn require_this_space(state: &AppState, space: &str) -> Result<(), ApiError> {
    if space != state.authority.space_uri() {
        return Err(ApiError::invalid_request(format!(
            "space not hosted here: {space}"
        )));
    }
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"version": env!("CARGO_PKG_VERSION")}))
}

async fn get_space(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<GetSpaceParams>,
) -> Result<Json<GetSpaceOutput>, ApiError> {
    require_space_credential(&state, &headers)?;
    require_this_space(&state, &params.space)?;
    Ok(Json(GetSpaceOutput {
        space: state.authority.space_uri(),
        config: SpaceConfig::Simplespace(state.authority.space_config(&state.policy)),
    }))
}

async fn get_space_credential(
    State(state): State<AppState>,
    Json(input): Json<GetSpaceCredentialInput>,
) -> Result<Json<GetSpaceCredentialOutput>, ApiError> {
    require_this_space(&state, &input.space)?;
    let credential = state
        .authority
        .get_space_credential(
            &input.delegation_token,
            input.client_attestation.as_deref(),
            &state.policy,
            state.keys.as_ref(),
            state.metadata.as_ref(),
            state.jti_store.as_ref(),
            (state.now)(),
            (state.jti)(),
        )
        .await?;
    Ok(Json(GetSpaceCredentialOutput { credential }))
}

async fn list_repos(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ListReposParams>,
) -> Result<Json<ListReposOutput>, ApiError> {
    require_space_credential(&state, &headers)?;
    require_this_space(&state, &params.space)?;
    let limit = params
        .limit
        .unwrap_or(DEFAULT_LIST_LIMIT)
        .clamp(1, MAX_LIST_LIMIT) as u32;
    let (repos, cursor) = state
        .writers
        .list_writers(&params.space, params.cursor.as_deref(), limit)
        .await?;
    Ok(Json(ListReposOutput { cursor, repos }))
}

async fn register_notify(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<RegisterNotifyInput>,
) -> Result<Json<RegisterNotifyOutput>, ApiError> {
    require_space_credential(&state, &headers)?;
    require_this_space(&state, &input.space)?;
    require_https(&input.endpoint)
        .map_err(|_| ApiError::invalid_request("endpoint must be https"))?;
    let expires_at = (state.now)() + state.registration_ttl_secs;
    state
        .registrations
        .register(&input.space, &input.endpoint, expires_at)
        .await?;
    Ok(Json(RegisterNotifyOutput {
        expires_at: chrono::DateTime::from_timestamp(expires_at as i64, 0).unwrap_or_default(),
    }))
}

async fn notify_write(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<NotifyWriteInput>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_this_space(&state, &input.space)?;
    let jwt = bearer(&headers)?;
    let claims = service_jwt::claims(jwt)?;
    // The repo host signs with the member's own key, so a notification may only
    // announce the issuer's own repo.
    if claims.iss != input.did {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "InvalidToken",
            "issuer does not match notified repo",
        ));
    }
    let issuer_key = state.keys.signing_key(&claims.iss).await?;
    let authority_did = state.authority.authority_did();
    let space_host_aud = format!("{authority_did}#atproto_space_host");
    service_jwt::verify(
        jwt,
        &[authority_did, space_host_aud.as_str()],
        NOTIFY_WRITE_LXM,
        &issuer_key,
        (state.now)(),
    )?;

    let now = (state.now)();
    state
        .writers
        .upsert_writer(&input.space, &input.did, &input.rev, None, now)
        .await?;
    let endpoints = state.registrations.endpoints(&input.space, now).await?;
    fan_out_write(state.notifier.clone(), endpoints, input);
    Ok(Json(serde_json::json!({})))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appaccess::AppAccess;
    use crate::attestation::{ClientMetadata, InMemoryJtiStore};
    use crate::error::Result as HostResult;
    use crate::membership::InMemoryMembership;
    use crate::signing::{test_signer, Signer};
    use crate::store::{InMemoryRegistrations, InMemoryWriterSet};
    use async_trait::async_trait;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use rsky_lexicon::com::atproto::space::NotifySpaceDeletedInput;
    use rsky_space::credential::{encode, JwtHeader, SpaceClaims, DELEGATION_TYP};
    use rsky_space::jwk::JwkSet;
    use rsky_space::space_id::SpaceId;
    use tower::ServiceExt;

    const NOW: u64 = 1000;
    const MEMBER: &str = "did:plc:member";

    fn space_uri() -> String {
        "at://did:plc:communityauthority/space/community.blacksky.feed/main".to_string()
    }

    fn user_signer() -> Signer {
        Signer::from_secret(secp256k1::SecretKey::from_slice(&[0x77u8; 32]).unwrap())
    }

    struct UserKeys;
    #[async_trait]
    impl KeyResolver for UserKeys {
        async fn signing_key(&self, did: &str) -> HostResult<String> {
            if did == MEMBER {
                Ok(user_signer().did_key().to_string())
            } else {
                Err(HostError::Resolution(format!("unknown did {did}")))
            }
        }
    }

    struct NoFetch;
    #[async_trait]
    impl MetadataFetcher for NoFetch {
        async fn client_metadata(&self, _client_id: &str) -> HostResult<ClientMetadata> {
            Err(HostError::Attestation("no metadata".into()))
        }
        async fn jwks(&self, _url: &str) -> HostResult<JwkSet> {
            Err(HostError::Attestation("no jwks".into()))
        }
    }

    struct RecordingNotifier {
        tx: tokio::sync::mpsc::UnboundedSender<(String, NotifyWriteInput)>,
    }
    #[async_trait]
    impl Notifier for RecordingNotifier {
        async fn notify_write(&self, endpoint: &str, input: &NotifyWriteInput) -> HostResult<()> {
            self.tx.send((endpoint.to_string(), input.clone())).unwrap();
            Ok(())
        }
        async fn notify_space_deleted(
            &self,
            _endpoint: &str,
            _input: &NotifySpaceDeletedInput,
        ) -> HostResult<()> {
            Ok(())
        }
    }

    struct BrokenWriters;
    #[async_trait]
    impl WriterSetStore for BrokenWriters {
        async fn upsert_writer(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: Option<&str>,
            _: u64,
        ) -> HostResult<()> {
            Err(HostError::Store("db down".into()))
        }
        async fn list_writers(
            &self,
            _: &str,
            _: Option<&str>,
            _: u32,
        ) -> HostResult<(
            Vec<rsky_lexicon::com::atproto::space::RepoRef>,
            Option<String>,
        )> {
            Err(HostError::Store("db down".into()))
        }
    }

    struct Fixture {
        state: AppState,
        writes: tokio::sync::mpsc::UnboundedReceiver<(String, NotifyWriteInput)>,
    }

    fn fixture(app_access: AppAccess, members: &[&str]) -> Fixture {
        let space = SpaceId::new(
            "did:plc:communityauthority",
            "community.blacksky.feed",
            "main",
        );
        let authority = Authority::new(space, test_signer(), app_access);
        let (tx, writes) = tokio::sync::mpsc::unbounded_channel();
        let state = AppState {
            authority: Arc::new(authority),
            policy: Arc::new(Policy::MemberList(Arc::new(InMemoryMembership::new(
                members.iter().map(|m| m.to_string()),
            )))),
            keys: Arc::new(UserKeys),
            metadata: Arc::new(NoFetch),
            jti_store: Arc::new(InMemoryJtiStore::default()),
            writers: Arc::new(InMemoryWriterSet::default()),
            registrations: Arc::new(InMemoryRegistrations::default()),
            notifier: Arc::new(RecordingNotifier { tx }),
            now: Arc::new(|| NOW),
            jti: Arc::new(|| "jti-fixed".to_string()),
            registration_ttl_secs: DEFAULT_REGISTRATION_TTL_SECS,
        };
        Fixture { state, writes }
    }

    fn credential_for(state: &AppState) -> String {
        state
            .authority
            .mint_credential(NOW, "cred-jti".to_string())
            .unwrap()
    }

    fn delegation_for(state: &AppState, user: &str) -> String {
        let header = JwtHeader {
            typ: DELEGATION_TYP.to_string(),
            alg: rsky_crypto::constants::SECP256K1_JWT_ALG.to_string(),
            kid: Some("#atproto".to_string()),
        };
        let claims = SpaceClaims {
            iss: user.to_string(),
            sub: state.authority.space_uri(),
            aud: Some(format!(
                "{}#atproto_space_host",
                state.authority.authority_did()
            )),
            iat: NOW,
            exp: NOW + 60,
            jti: "delegation-jti".to_string(),
        };
        encode(&header, &claims, |input| user_signer().sign(input)).unwrap()
    }

    fn member_service_jwt(aud: &str, lxm: &str) -> String {
        service_jwt::mint(&user_signer(), MEMBER, aud, lxm, NOW, "svc-jti".to_string()).unwrap()
    }

    async fn send(state: &AppState, request: Request<Body>) -> (StatusCode, serde_json::Value) {
        let response = router(state.clone()).oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    fn get_req(path: &str, token: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder().method("GET").uri(path);
        if let Some(token) = token {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        builder.body(Body::empty()).unwrap()
    }

    fn post_req(path: &str, token: Option<&str>, body: serde_json::Value) -> Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json");
        if let Some(token) = token {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    #[tokio::test]
    async fn health_reports_version() {
        let f = fixture(AppAccess::Open, &[]);
        let (status, body) = send(&f.state, get_req("/xrpc/_health", None)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn get_space_requires_and_verifies_a_credential() {
        let f = fixture(AppAccess::Open, &[]);
        let path = format!(
            "/xrpc/com.atproto.space.getSpace?space={}",
            urlencode(&space_uri())
        );

        let (status, body) = send(&f.state, get_req(&path, None)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "AuthenticationRequired");

        let (status, body) = send(&f.state, get_req(&path, Some("garbage"))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "InvalidToken");

        let token = credential_for(&f.state);
        let (status, body) = send(&f.state, get_req(&path, Some(&token))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["space"], space_uri());
        assert_eq!(
            body["config"]["$type"],
            "com.atproto.simplespace.defs#config"
        );
        assert_eq!(body["config"]["policy"], "member-list");

        let wrong = format!(
            "/xrpc/com.atproto.space.getSpace?space={}",
            urlencode("at://did:plc:other/space/community.blacksky.feed/main")
        );
        let (status, body) = send(&f.state, get_req(&wrong, Some(&token))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "InvalidRequest");
    }

    #[tokio::test]
    async fn get_space_credential_happy_and_denied_paths() {
        let f = fixture(AppAccess::Open, &[MEMBER]);
        let path = "/xrpc/com.atproto.space.getSpaceCredential";

        let body = serde_json::json!({
            "space": space_uri(),
            "delegationToken": delegation_for(&f.state, MEMBER),
        });
        let (status, out) = send(&f.state, post_req(path, None, body)).await;
        assert_eq!(status, StatusCode::OK);
        credential::verify_space_credential(
            out["credential"].as_str().unwrap(),
            &space_uri(),
            f.state.authority.authority_did(),
            f.state.authority.signer.did_key(),
            NOW,
        )
        .unwrap();

        // Non-member is refused by the policy.
        let f = fixture(AppAccess::Open, &[]);
        let body = serde_json::json!({
            "space": space_uri(),
            "delegationToken": delegation_for(&f.state, MEMBER),
        });
        let (status, out) = send(&f.state, post_req(path, None, body)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(out["error"], "NotAuthorized");

        // Garbage delegation token.
        let body = serde_json::json!({
            "space": space_uri(),
            "delegationToken": "garbage",
        });
        let (status, out) = send(&f.state, post_req(path, None, body)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(out["error"], "InvalidToken");

        // Wrong space.
        let body = serde_json::json!({
            "space": "at://did:plc:other/space/community.blacksky.feed/main",
            "delegationToken": "x",
        });
        let (status, out) = send(&f.state, post_req(path, None, body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(out["error"], "InvalidRequest");
    }

    #[tokio::test]
    async fn get_space_credential_attestation_errors() {
        let f = fixture(
            AppAccess::AllowList(vec!["https://app.example/client".to_string()]),
            &[MEMBER],
        );
        let path = "/xrpc/com.atproto.space.getSpaceCredential";

        let body = serde_json::json!({
            "space": space_uri(),
            "delegationToken": delegation_for(&f.state, MEMBER),
        });
        let (status, out) = send(&f.state, post_req(path, None, body)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(out["error"], "AttestationRequired");

        let body = serde_json::json!({
            "space": space_uri(),
            "delegationToken": delegation_for(&f.state, MEMBER),
            "clientAttestation": "garbage",
        });
        let (status, out) = send(&f.state, post_req(path, None, body)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(out["error"], "InvalidAttestation");
    }

    #[tokio::test]
    async fn list_repos_pages_the_writer_set() {
        let f = fixture(AppAccess::Open, &[]);
        for did in ["did:plc:aaa", "did:plc:bbb", "did:plc:ccc"] {
            f.state
                .writers
                .upsert_writer(&space_uri(), did, "rev1", Some("h"), NOW)
                .await
                .unwrap();
        }
        let token = credential_for(&f.state);
        let base = format!(
            "/xrpc/com.atproto.space.listRepos?space={}",
            urlencode(&space_uri())
        );

        let (status, _) = send(&f.state, get_req(&base, None)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, body) =
            send(&f.state, get_req(&format!("{base}&limit=2"), Some(&token))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["repos"].as_array().unwrap().len(), 2);
        assert_eq!(body["cursor"], "did:plc:bbb");

        let (status, body) = send(
            &f.state,
            get_req(&format!("{base}&limit=2&cursor=did:plc:bbb"), Some(&token)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["repos"].as_array().unwrap().len(), 1);
        assert_eq!(body["repos"][0]["did"], "did:plc:ccc");
        assert!(body["cursor"].is_null());

        // Out-of-range limits are clamped rather than erroring.
        let (status, body) =
            send(&f.state, get_req(&format!("{base}&limit=0"), Some(&token))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["repos"].as_array().unwrap().len(), 1);

        // Store failures surface as InternalError.
        let mut broken = f.state.clone();
        broken.writers = Arc::new(BrokenWriters);
        let (status, body) = send(&broken, get_req(&base, Some(&token))).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"], "InternalError");
    }

    #[tokio::test]
    async fn register_notify_stores_an_expiring_registration() {
        let f = fixture(AppAccess::Open, &[]);
        let token = credential_for(&f.state);
        let path = "/xrpc/com.atproto.space.registerNotify";
        let body = serde_json::json!({
            "space": space_uri(),
            "endpoint": "https://syncer.example",
        });

        let (status, _) = send(&f.state, post_req(path, None, body.clone())).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, out) = send(&f.state, post_req(path, Some(&token), body)).await;
        assert_eq!(status, StatusCode::OK);
        let expires_at = out["expiresAt"].as_str().unwrap();
        let parsed = chrono::DateTime::parse_from_rfc3339(expires_at).unwrap();
        assert_eq!(
            parsed.timestamp() as u64,
            NOW + DEFAULT_REGISTRATION_TTL_SECS
        );
        let endpoints = f
            .state
            .registrations
            .endpoints(&space_uri(), NOW)
            .await
            .unwrap();
        assert_eq!(endpoints, vec!["https://syncer.example".to_string()]);

        // Non-https endpoints are rejected.
        let body = serde_json::json!({
            "space": space_uri(),
            "endpoint": "http://syncer.example",
        });
        let (status, out) = send(&f.state, post_req(path, Some(&token), body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(out["error"], "InvalidRequest");
    }

    #[tokio::test]
    async fn notify_write_updates_writers_and_fans_out() {
        let mut f = fixture(AppAccess::Open, &[]);
        f.state
            .registrations
            .register(&space_uri(), "https://syncer.example", NOW + 100)
            .await
            .unwrap();
        let aud = format!("{}#atproto_space_host", f.state.authority.authority_did());
        let token = member_service_jwt(&aud, NOTIFY_WRITE_LXM);
        let path = "/xrpc/com.atproto.space.notifyWrite";
        let body = serde_json::json!({
            "space": space_uri(),
            "did": MEMBER,
            "rev": "3jzfcijpj2z2c",
        });

        let (status, _) = send(&f.state, post_req(path, Some(&token), body)).await;
        assert_eq!(status, StatusCode::OK);

        let (repos, _) = f
            .state
            .writers
            .list_writers(&space_uri(), None, 10)
            .await
            .unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].did, MEMBER);
        assert_eq!(repos[0].rev, "3jzfcijpj2z2c");

        let (endpoint, forwarded) = f.writes.recv().await.unwrap();
        assert_eq!(endpoint, "https://syncer.example");
        assert_eq!(forwarded.did, MEMBER);
    }

    #[tokio::test]
    async fn notify_write_auth_failures() {
        let f = fixture(AppAccess::Open, &[]);
        let path = "/xrpc/com.atproto.space.notifyWrite";
        let authority_did = f.state.authority.authority_did().to_string();
        let body = serde_json::json!({
            "space": space_uri(),
            "did": MEMBER,
            "rev": "3jzfcijpj2z2c",
        });

        // Missing token.
        let (status, out) = send(&f.state, post_req(path, None, body.clone())).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(out["error"], "AuthenticationRequired");

        // Issuer announcing someone else's repo.
        let token = member_service_jwt(&authority_did, NOTIFY_WRITE_LXM);
        let other = serde_json::json!({
            "space": space_uri(),
            "did": "did:plc:someoneelse",
            "rev": "3jzfcijpj2z2c",
        });
        let (status, out) = send(&f.state, post_req(path, Some(&token), other)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(out["error"], "InvalidToken");

        // Wrong audience.
        let token = member_service_jwt("did:plc:other", NOTIFY_WRITE_LXM);
        let (status, out) = send(&f.state, post_req(path, Some(&token), body.clone())).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(out["error"], "InvalidToken");

        // Wrong space.
        let token = member_service_jwt(&authority_did, NOTIFY_WRITE_LXM);
        let wrong_space = serde_json::json!({
            "space": "at://did:plc:other/space/community.blacksky.feed/main",
            "did": MEMBER,
            "rev": "3jzfcijpj2z2c",
        });
        let (status, out) = send(&f.state, post_req(path, Some(&token), wrong_space)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(out["error"], "InvalidRequest");

        // Unresolvable issuer key.
        let stranger =
            Signer::from_secret(secp256k1::SecretKey::from_slice(&[0x88u8; 32]).unwrap());
        let token = service_jwt::mint(
            &stranger,
            "did:plc:unknown",
            &authority_did,
            NOTIFY_WRITE_LXM,
            NOW,
            "svc-jti".to_string(),
        )
        .unwrap();
        let unknown = serde_json::json!({
            "space": space_uri(),
            "did": "did:plc:unknown",
            "rev": "3jzfcijpj2z2c",
        });
        let (status, out) = send(&f.state, post_req(path, Some(&token), unknown)).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(out["error"], "InternalError");
    }

    #[tokio::test]
    async fn notify_write_store_failure_is_internal_error() {
        let f = fixture(AppAccess::Open, &[]);
        let mut broken = f.state.clone();
        broken.writers = Arc::new(BrokenWriters);
        let token = member_service_jwt(
            &format!("{}#atproto_space_host", broken.authority.authority_did()),
            NOTIFY_WRITE_LXM,
        );
        let body = serde_json::json!({
            "space": space_uri(),
            "did": MEMBER,
            "rev": "3jzfcijpj2z2c",
        });
        let (status, out) = send(
            &broken,
            post_req("/xrpc/com.atproto.space.notifyWrite", Some(&token), body),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(out["error"], "InternalError");
    }

    #[tokio::test]
    async fn error_mapping_covers_every_lexicon_error_name() {
        for (err, status, name) in [
            (
                HostError::Delegation("bad".into()),
                StatusCode::UNAUTHORIZED,
                "InvalidToken",
            ),
            (
                HostError::AttestationRequired,
                StatusCode::UNAUTHORIZED,
                "AttestationRequired",
            ),
            (
                HostError::Attestation("bad".into()),
                StatusCode::UNAUTHORIZED,
                "InvalidAttestation",
            ),
            (
                HostError::NotAuthorized,
                StatusCode::FORBIDDEN,
                "NotAuthorized",
            ),
            (
                HostError::ClientNotAuthorized,
                StatusCode::FORBIDDEN,
                "ClientNotAuthorized",
            ),
            (
                HostError::Store("down".into()),
                StatusCode::INTERNAL_SERVER_ERROR,
                "InternalError",
            ),
        ] {
            let api = ApiError::from(err);
            let response = api.into_response();
            assert_eq!(response.status(), status);
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(body["error"], name);
        }
    }

    #[tokio::test]
    async fn test_doubles_behave_as_declared() {
        assert!(NoFetch.client_metadata("x").await.is_err());
        assert!(NoFetch.jwks("x").await.is_err());
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let notifier = RecordingNotifier { tx };
        notifier
            .notify_space_deleted(
                "https://syncer.example",
                &NotifySpaceDeletedInput { space: space_uri() },
            )
            .await
            .unwrap();
    }

    fn urlencode(s: &str) -> String {
        s.replace(':', "%3A").replace('/', "%2F")
    }
}
