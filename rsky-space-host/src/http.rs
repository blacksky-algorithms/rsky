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
use rsky_oauth::dpop::{DpopManager, DpopProof, DpopRequest};
use rsky_space::credential;
use std::sync::Arc;

use crate::attestation::{JtiStore, MetadataFetcher};
use crate::authority::{Authority, KeyResolver};
use crate::error::HostError;
use crate::keys::DocSource;
use crate::managing_app::require_https;
use crate::notify::{fan_out_write, Notifier, NOTIFY_WRITE_LXM};
use crate::policy::Policy;
use crate::registration::{LifecycleAcker, REGISTER_SPACE_LXM};
use crate::service_jwt;
use crate::store::{RegistrationStore, Subscriber, WriterSetStore};

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
    pub lifecycle_acker: Option<Arc<dyn LifecycleAcker>>,
    /// Resolves a subscriber's service identifier to its delivery endpoint.
    pub docs: Arc<dyn DocSource>,
    pub notifier: Arc<dyn Notifier>,
    /// Verifies DPoP proofs on the credential-issuance and credential-presenting
    /// paths. Space issuance does not challenge with nonces, so this manager
    /// carries none; the replay store is what makes a proof single-use.
    pub dpop: Arc<DpopManager>,
    /// Public origin proofs must be bound to.
    pub public_url: String,
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
        .route(
            "/xrpc/community.blacksky.space.register",
            post(register_space),
        )
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

    fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "Forbidden", message)
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
            HostError::SpaceNotFound(_) => Self::new(
                StatusCode::NOT_FOUND,
                "SpaceNotFound",
                "space not hosted here",
            ),
            HostError::RepoNotFound => {
                Self::new(StatusCode::NOT_FOUND, "RepoNotFound", "repo not found")
            }
            HostError::InvalidRequest(message) => Self::invalid_request(message.clone()),
            HostError::InvalidSwap => Self::new(
                StatusCode::CONFLICT,
                "InvalidSwap",
                "swap cid did not match",
            ),
            HostError::HistoryUnavailable => Self::new(
                StatusCode::GONE,
                "HistoryUnavailable",
                "requested history is no longer available",
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

fn dpop_credential(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("DPoP "))
        .ok_or_else(|| ApiError::auth_required("missing DPoP-bound space credential"))
}

/// Check the request's DPoP proof (RFC 9449). `access_token` is the credential
/// the proof must hash into `ath`, and is absent at issuance, where the
/// delegation token is a grant rather than a bound token.
fn check_proof(
    state: &AppState,
    headers: &HeaderMap,
    method: &str,
    nsid: &str,
    access_token: Option<&str>,
) -> Result<DpopProof, ApiError> {
    let uri = format!("{}/xrpc/{nsid}", state.public_url.trim_end_matches('/'));
    let proofs: Vec<&str> = headers
        .get_all("dpop")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect();
    state
        .dpop
        .check_proof(
            &DpopRequest {
                method,
                uri: &uri,
                dpop_headers: &proofs,
                access_token,
            },
            (state.now)(),
        )
        .map_err(|e| ApiError::new(StatusCode::UNAUTHORIZED, "InvalidDpopProof", e.to_string()))?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "InvalidDpopProof",
                "missing DPoP proof",
            )
        })
}

/// Space-credential auth: verify the presented credential against this
/// authority's own space key, then confirm the presenter holds the key it is
/// bound to.
///
/// A credential reads every repo in its space and is presented to each of
/// their hosts in turn, so as a bearer token it would be a shared secret any
/// one of those hosts could replay against the others. `Bearer` is refused
/// even with a valid proof beside it.
fn require_space_credential(
    state: &AppState,
    headers: &HeaderMap,
    method: &str,
    nsid: &str,
    space_uri: &str,
) -> Result<rsky_space::space_id::SpaceId, ApiError> {
    let space = require_this_space(state, space_uri)?;
    let jwt = dpop_credential(headers)?;
    let bound_jkt = credential::verify_space_credential(
        jwt,
        &space.uri(),
        state.authority.authority_did(),
        state.authority.signer.did_key(),
        (state.now)(),
    )
    .map_err(|e| ApiError::new(StatusCode::UNAUTHORIZED, "InvalidToken", e.to_string()))?;
    let proof = check_proof(state, headers, method, nsid, Some(jwt))?;
    if proof.jkt != bound_jkt {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "InvalidDpopProof",
            "DPoP key thumbprint does not match the credential binding",
        ));
    }
    Ok(space)
}

/// Resolve a `did:...#fragment` subscriber to its delivery endpoint. The
/// fragment defaults to `#atproto_space_syncer`, the entry a subscriber
/// publishes for this purpose.
async fn resolve_service_endpoint(docs: &dyn DocSource, service: &str) -> Result<String, ApiError> {
    let (did, fragment) = match service.split_once('#') {
        Some((did, fragment)) => (did, fragment),
        None => (service, "atproto_space_syncer"),
    };
    let doc = docs.did_document(did).await.map_err(|e| {
        ApiError::invalid_request(format!("could not resolve service {service}: {e}"))
    })?;
    doc.service
        .as_deref()
        .unwrap_or_default()
        .iter()
        .find(|entry| {
            entry.id.rsplit_once('#').map(|(_, f)| f) == Some(fragment) || entry.id == fragment
        })
        .map(|entry| entry.service_endpoint.clone())
        .ok_or_else(|| {
            ApiError::invalid_request(format!(
                "no {fragment} service in the DID document for {did}"
            ))
        })
}

fn require_this_space(
    state: &AppState,
    space: &str,
) -> Result<rsky_space::space_id::SpaceId, ApiError> {
    state
        .authority
        .resolve(space)
        .map_err(|_| ApiError::invalid_request(format!("space not hosted here: {space}")))
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"version": env!("CARGO_PKG_VERSION")}))
}

async fn get_space(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<GetSpaceParams>,
) -> Result<Json<GetSpaceOutput>, ApiError> {
    let space = require_space_credential(
        &state,
        &headers,
        "GET",
        "com.atproto.space.getSpace",
        &params.space,
    )?;
    Ok(Json(GetSpaceOutput {
        space: space.uri(),
        config: SpaceConfig::Simplespace(state.authority.space_config(&state.policy)),
    }))
}

async fn require_service_auth(
    state: &AppState,
    headers: &HeaderMap,
    expected_lxm: &str,
) -> Result<crate::service_jwt::ServiceClaims, ApiError> {
    let jwt = bearer(headers)?;
    let claims = service_jwt::claims(jwt)?;
    let issuer_key = state.keys.signing_key(&claims.iss).await?;
    let authority_did = state.authority.authority_did();
    let space_host_aud = format!("{authority_did}#atproto_space_host");
    service_jwt::verify(
        jwt,
        &[authority_did, space_host_aud.as_str()],
        expected_lxm,
        &issuer_key,
        (state.now)(),
    )
    .map_err(|e| ApiError::new(StatusCode::UNAUTHORIZED, "InvalidToken", e.to_string()))
}

async fn get_space_credential(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<GetSpaceCredentialInput>,
) -> Result<Json<GetSpaceCredentialOutput>, ApiError> {
    let space = require_this_space(&state, &input.space)?;
    let delegation_token = bearer(&headers)?;
    // Before the delegation token, so a caller with a bad proof does not burn
    // its single-use grant finding out.
    let proof = check_proof(
        &state,
        &headers,
        "POST",
        "com.atproto.space.getSpaceCredential",
        None,
    )?;
    let credential = state
        .authority
        .get_space_credential_for(
            &space,
            delegation_token,
            input.client_attestation.as_deref(),
            &state.policy,
            state.keys.as_ref(),
            state.metadata.as_ref(),
            state.jti_store.as_ref(),
            (state.now)(),
            (state.jti)(),
            &proof.jkt,
        )
        .await?;
    Ok(Json(GetSpaceCredentialOutput { credential }))
}

async fn list_repos(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ListReposParams>,
) -> Result<Json<ListReposOutput>, ApiError> {
    require_space_credential(
        &state,
        &headers,
        "GET",
        "com.atproto.space.listRepos",
        &params.space,
    )?;
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
    require_space_credential(
        &state,
        &headers,
        "POST",
        "com.atproto.space.registerNotify",
        &input.space,
    )?;
    // `service` names the subscriber, which is both where to deliver and who
    // the delivery is addressed to (proposals#100); `endpoint` is the
    // pre-amendment shape and loses when both are sent.
    let subscriber = match (&input.service, &input.endpoint) {
        (Some(service), _) => Subscriber {
            endpoint: resolve_service_endpoint(state.docs.as_ref(), service).await?,
            service: Some(service.clone()),
        },
        (None, Some(endpoint)) => {
            require_https(endpoint)
                .map_err(|_| ApiError::invalid_request("endpoint must be https"))?;
            Subscriber {
                endpoint: endpoint.clone(),
                service: None,
            }
        }
        (None, None) => {
            return Err(ApiError::invalid_request(
                "registerNotify requires a service or an endpoint",
            ))
        }
    };
    let expires_at = (state.now)() + state.registration_ttl_secs;
    state
        .registrations
        .register(&input.space, &subscriber, expires_at)
        .await?;
    Ok(Json(RegisterNotifyOutput {
        expires_at: chrono::DateTime::from_timestamp(expires_at as i64, 0).unwrap_or_default(),
    }))
}

#[derive(serde::Deserialize)]
struct RegisterSpaceInput {
    space: String,
    generation: i64,
}

async fn register_space(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<RegisterSpaceInput>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if input.generation < 1 {
        return Err(ApiError::invalid_request("generation must be positive"));
    }
    let claims = require_service_auth(&state, &headers, REGISTER_SPACE_LXM).await?;
    let expected_issuer = state
        .policy
        .managing_app()
        .and_then(|service| service.split_once('#').map(|(did, _)| did))
        .ok_or_else(|| ApiError::forbidden("space has no managing app"))?;
    if claims.iss != expected_issuer {
        return Err(ApiError::forbidden("issuer is not the managing app"));
    }
    let acker = state.lifecycle_acker.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "LifecycleUnavailable",
            "lifecycle acknowledgement is not configured",
        )
    })?;
    state.authority.register(&input.space)?;
    acker
        .ack_host_registered(&input.space, input.generation)
        .await?;
    Ok(Json(serde_json::json!({})))
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
    if claims.iss != input.repo {
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
        .upsert_writer(&input.space, &input.repo, &input.rev, None, now)
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
    const PUBLIC_URL: &str = "https://space.example";
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
        async fn notify_write(&self, to: &Subscriber, input: &NotifyWriteInput) -> HostResult<()> {
            self.tx
                .send((to.audience().to_string(), input.clone()))
                .unwrap();
            Ok(())
        }
        async fn notify_space_deleted(
            &self,
            _to: &Subscriber,
            _input: &NotifySpaceDeletedInput,
        ) -> HostResult<()> {
            Ok(())
        }
    }

    /// The tests never register by service identifier, so nothing here should
    /// reach DID resolution -- and a test that starts to will say so.
    struct NoDocs;
    #[async_trait]
    impl crate::keys::DocSource for NoDocs {
        async fn did_document(&self, did: &str) -> HostResult<rsky_identity::types::DidDocument> {
            Err(HostError::Resolution(format!("no document for {did}")))
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

    #[derive(Default)]
    struct RecordingAcker(std::sync::Mutex<Vec<(String, i64)>>);

    #[async_trait]
    impl LifecycleAcker for RecordingAcker {
        async fn ack_host_registered(&self, space: &str, generation: i64) -> HostResult<()> {
            self.0.lock().unwrap().push((space.to_string(), generation));
            Ok(())
        }
    }

    struct UnusedManagingApp;

    #[async_trait]
    impl crate::managing_app::ManagingAppClient for UnusedManagingApp {
        async fn check_user_access(&self, _: &str, _: &str, _: Option<&str>) -> HostResult<bool> {
            Ok(false)
        }
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
            lifecycle_acker: None,
            docs: Arc::new(NoDocs),
            notifier: Arc::new(RecordingNotifier { tx }),
            dpop: Arc::new(rsky_oauth::dpop::DpopManager::new(
                None,
                Box::new(rsky_oauth::dpop::InMemoryReplayStore::default()),
            )),
            public_url: PUBLIC_URL.to_string(),
            now: Arc::new(|| NOW),
            jti: Arc::new(|| "jti-fixed".to_string()),
            registration_ttl_secs: DEFAULT_REGISTRATION_TTL_SECS,
        };
        Fixture { state, writes }
    }

    const TEST_DPOP_KEY: [u8; 32] = [0x42u8; 32];
    static PROOF_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn dpop_key() -> rsky_oauth::jwk::Jwk {
        rsky_oauth::jwk::Jwk::from_private_key_bytes(rsky_oauth::jwk::EcCurve::P256, &TEST_DPOP_KEY)
            .unwrap()
    }

    /// A proof over `{PUBLIC_URL}/xrpc/{nsid}`. `access_token` is the bound
    /// credential when there is one; issuance has none.
    fn dpop_proof(method: &str, nsid: &str, access_token: Option<&str>) -> String {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine as _;
        use sha2::Digest as _;
        let key = dpop_key();
        let mut header = rsky_oauth::jwt::JwtHeader::new("ES256");
        header.typ = Some("dpop+jwt".to_string());
        header.jwk = Some(key.to_public());
        let mut claims = rsky_oauth::jwt::JwtClaims {
            iat: Some(NOW),
            jti: Some(format!(
                "proof-{}",
                PROOF_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            )),
            ..Default::default()
        };
        claims.extra.insert("htm".to_string(), method.into());
        claims.extra.insert(
            "htu".to_string(),
            format!("{PUBLIC_URL}/xrpc/{nsid}").into(),
        );
        if let Some(token) = access_token {
            let ath = URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(token.as_bytes()));
            claims.extra.insert("ath".to_string(), ath.into());
        }
        rsky_oauth::jwt::sign(&header, &claims, &key).unwrap()
    }

    fn credential_for(state: &AppState) -> String {
        state
            .authority
            .mint_credential(NOW, "cred-jti".to_string(), &dpop_key().thumbprint())
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
            cnf: None,
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

    /// Attach the credential the way the server now demands it: a space
    /// credential under `DPoP` with a matching proof, a delegation token as a
    /// `Bearer` grant with a proof carrying no `ath`, anything else as a plain
    /// bearer token.
    fn with_auth(
        mut builder: axum::http::request::Builder,
        method: &str,
        path: &str,
        token: Option<&str>,
    ) -> axum::http::request::Builder {
        let Some(token) = token else {
            return builder;
        };
        let nsid = path
            .trim_start_matches("/xrpc/")
            .split('?')
            .next()
            .unwrap_or_default()
            .to_string();
        match credential::decode(token).map(|d| d.header.typ) {
            Ok(typ) if typ == credential::CREDENTIAL_TYP => {
                builder = builder.header("authorization", format!("DPoP {token}"));
                builder.header("dpop", dpop_proof(method, &nsid, Some(token)))
            }
            Ok(typ) if typ == DELEGATION_TYP => {
                builder = builder.header("authorization", format!("Bearer {token}"));
                builder.header("dpop", dpop_proof(method, &nsid, None))
            }
            _ => builder.header("authorization", format!("Bearer {token}")),
        }
    }

    fn get_req(path: &str, token: Option<&str>) -> Request<Body> {
        let builder = Request::builder().method("GET").uri(path);
        with_auth(builder, "GET", path, token)
            .body(Body::empty())
            .unwrap()
    }

    fn post_req(path: &str, token: Option<&str>, body: serde_json::Value) -> Request<Body> {
        let builder = Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json");
        with_auth(builder, "POST", path, token)
            .body(Body::from(body.to_string()))
            .unwrap()
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

        // A bearer token is not a credential presentation, whatever it holds.
        let (status, body) = send(&f.state, get_req(&path, Some("garbage"))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "AuthenticationRequired");

        let token = credential_for(&f.state);

        // Not even a real credential with a valid proof beside it: the scheme
        // is the refusal, so it cannot be used to opt out of the binding.
        let as_bearer = Request::builder()
            .method("GET")
            .uri(&path)
            .header("authorization", format!("Bearer {token}"))
            .header(
                "dpop",
                dpop_proof("GET", "com.atproto.space.getSpace", Some(&token)),
            )
            .body(Body::empty())
            .unwrap();
        let (status, body) = send(&f.state, as_bearer).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "AuthenticationRequired");

        // A credential bound to someone else's key is refused as well.
        let other_binding = f
            .state
            .authority
            .mint_credential(NOW, "other-jti".to_string(), "some-other-thumbprint")
            .unwrap();
        let (status, body) = send(&f.state, get_req(&path, Some(&other_binding))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "InvalidDpopProof");

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

        let body = serde_json::json!({ "space": space_uri() });
        let delegation = delegation_for(&f.state, MEMBER);
        let (status, out) = send(&f.state, post_req(path, Some(&delegation), body)).await;
        assert_eq!(status, StatusCode::OK);
        credential::verify_space_credential(
            out["credential"].as_str().unwrap(),
            &space_uri(),
            f.state.authority.authority_did(),
            f.state.authority.signer.did_key(),
            NOW,
        )
        .unwrap();

        // A credential is minted only for a proof-carrying request: the key
        // it binds to is established by the proof, so there is nothing to bind
        // without one.
        let unbound = Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {delegation}"))
            .body(Body::from(
                serde_json::json!({ "space": space_uri() }).to_string(),
            ))
            .unwrap();
        let (status, out) = send(&f.state, unbound).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(out["error"], "InvalidDpopProof");

        // Non-member is refused by the policy.
        let f = fixture(AppAccess::Open, &[]);
        let body = serde_json::json!({ "space": space_uri() });
        let delegation = delegation_for(&f.state, MEMBER);
        let (status, out) = send(&f.state, post_req(path, Some(&delegation), body)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(out["error"], "NotAuthorized");

        // Garbage delegation token.
        let garbage = Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json")
            .header("authorization", "Bearer garbage")
            .header(
                "dpop",
                dpop_proof("POST", "com.atproto.space.getSpaceCredential", None),
            )
            .body(Body::from(
                serde_json::json!({ "space": space_uri() }).to_string(),
            ))
            .unwrap();
        let (status, out) = send(&f.state, garbage).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(out["error"], "InvalidToken");

        // Wrong space.
        let body = serde_json::json!({
            "space": "at://did:plc:other/space/community.blacksky.feed/main",
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

        let delegation = delegation_for(&f.state, MEMBER);
        let body = serde_json::json!({ "space": space_uri() });
        let (status, out) = send(&f.state, post_req(path, Some(&delegation), body)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(out["error"], "AttestationRequired");

        let body = serde_json::json!({
            "space": space_uri(),
            "clientAttestation": "garbage",
        });
        let (status, out) = send(&f.state, post_req(path, Some(&delegation), body)).await;
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
        assert_eq!(
            endpoints,
            vec![Subscriber {
                endpoint: "https://syncer.example".to_string(),
                service: None,
            }]
        );

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
            .register(
                &space_uri(),
                &Subscriber {
                    endpoint: "https://syncer.example".to_string(),
                    service: None,
                },
                NOW + 100,
            )
            .await
            .unwrap();
        let aud = format!("{}#atproto_space_host", f.state.authority.authority_did());
        let token = member_service_jwt(&aud, NOTIFY_WRITE_LXM);
        let path = "/xrpc/com.atproto.space.notifyWrite";
        let body = serde_json::json!({
            "space": space_uri(),
            "repo": MEMBER,
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
        assert_eq!(forwarded.repo, MEMBER);
    }

    #[tokio::test]
    async fn notify_write_auth_failures() {
        let f = fixture(AppAccess::Open, &[]);
        let path = "/xrpc/com.atproto.space.notifyWrite";
        let authority_did = f.state.authority.authority_did().to_string();
        let body = serde_json::json!({
            "space": space_uri(),
            "repo": MEMBER,
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
            "repo": "did:plc:someoneelse",
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
            "repo": MEMBER,
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
            "repo": "did:plc:unknown",
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
            "repo": MEMBER,
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
    async fn registration_authenticates_the_managing_app_and_acks_before_activation() {
        let mut f = fixture(AppAccess::Open, &[]);
        f.state.policy = Arc::new(Policy::ManagingApp {
            service_id: format!("{MEMBER}#bsky_fg"),
            client: Arc::new(UnusedManagingApp),
        });
        let acker = Arc::new(RecordingAcker::default());
        f.state.lifecycle_acker = Some(acker.clone());
        let space = "at://did:plc:communityauthority/space/community.blacksky.feed/new";
        let audience = format!("{}#atproto_space_host", f.state.authority.authority_did());
        let token = service_jwt::mint(
            &user_signer(),
            MEMBER,
            &audience,
            REGISTER_SPACE_LXM,
            NOW,
            "register-1".to_string(),
        )
        .unwrap();
        let (status, body) = send(
            &f.state,
            post_req(
                &format!("/xrpc/{REGISTER_SPACE_LXM}"),
                Some(&token),
                serde_json::json!({"space": space, "generation": 7}),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(f.state.authority.resolve_registered(space).is_ok());
        assert_eq!(
            acker.0.lock().unwrap().as_slice(),
            &[(space.to_string(), 7)]
        );
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
                &Subscriber {
                    endpoint: "https://syncer.example".to_string(),
                    service: None,
                },
                &NotifySpaceDeletedInput { space: space_uri() },
            )
            .await
            .unwrap();
    }

    fn urlencode(s: &str) -> String {
        s.replace(':', "%3A").replace('/', "%2F")
    }
}
