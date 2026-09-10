//! The internal, non-XRPC space supervision contract.

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::engine::CommitKeyResolver;
use crate::error::{DaemonError, Result};
use crate::runner::SupervisorCommand;
use crate::sqlite_index::{DirectIntakeOutcome, SqliteIndex};

pub const INTAKE_METHOD: &str = "community.blacksky.internal.spaceIntake";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IntakeRequest {
    pub contract_version: u8,
    pub event_id: String,
    pub space: String,
    pub generation: i64,
    pub desired_state: String,
    pub lifecycle_state: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct IntakeResponse {
    contract_version: u8,
    event_id: String,
    space: String,
    generation: i64,
    desired_state: String,
    result: &'static str,
    persisted: bool,
}

#[derive(Debug, Deserialize)]
struct Claims {
    iss: String,
    aud: String,
    lxm: String,
    iat: u64,
    exp: u64,
    jti: String,
}

#[derive(Clone)]
pub struct IntakeState {
    pub db: Arc<SqliteIndex>,
    pub supervisor: mpsc::Sender<SupervisorCommand>,
    pub issuer_did: String,
    pub service_identity: String,
    pub space_type: String,
    pub resolver: Arc<dyn CommitKeyResolver>,
    pub now_fn: fn() -> u64,
}

pub fn router(state: IntakeState) -> Router {
    Router::new()
        .route("/internal/v1/spaces/intake", axum::routing::post(intake))
        .with_state(state)
}

pub fn event_id(space: &str, generation: i64, desired: &str, lifecycle: &str) -> String {
    let input = format!("acorn-daemon-intake-v1\0{space}\0{generation}\0{desired}\0{lifecycle}");
    hex::encode(Sha256::digest(input.as_bytes()))
}

pub fn payload_hash(request: &IntakeRequest) -> String {
    let encoded = serde_json::to_vec(request).expect("intake request is serializable");
    hex::encode(Sha256::digest(encoded))
}

fn error_body(error: &str, message: impl std::fmt::Display) -> Json<Value> {
    Json(json!({ "error": error, "message": message.to_string() }))
}

fn decode_claims(jwt: &str) -> Result<Claims> {
    let parts: Vec<_> = jwt.split('.').collect();
    if parts.len() != 3 {
        return Err(DaemonError::Xrpc("malformed JWT".into()));
    }
    let header: Value = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(parts[0])
            .map_err(|_| DaemonError::Xrpc("malformed JWT header".into()))?,
    )
    .map_err(|_| DaemonError::Xrpc("malformed JWT header".into()))?;
    if header.get("typ").and_then(Value::as_str) != Some("JWT")
        || header.get("alg").and_then(Value::as_str) != Some("ES256K")
    {
        return Err(DaemonError::Xrpc("unsupported JWT header".into()));
    }
    serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(parts[1])
            .map_err(|_| DaemonError::Xrpc("malformed JWT payload".into()))?,
    )
    .map_err(|_| DaemonError::Xrpc("malformed JWT claims".into()))
}

async fn authenticate(
    headers: &HeaderMap,
    state: &IntakeState,
    request: &IntakeRequest,
) -> Result<()> {
    let jwt = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| DaemonError::Xrpc("missing bearer token".into()))?;
    let claims = decode_claims(jwt)?;
    let now = (state.now_fn)();
    if claims.iss != state.issuer_did || claims.aud != state.service_identity {
        return Err(DaemonError::Xrpc("JWT issuer or audience mismatch".into()));
    }
    if claims.lxm != INTAKE_METHOD || claims.jti != request.event_id || claims.jti.is_empty() {
        return Err(DaemonError::Xrpc("JWT method or event mismatch".into()));
    }
    if claims.exp <= now
        || claims.iat > now.saturating_add(30)
        || claims.exp.saturating_sub(claims.iat) > 60
    {
        return Err(DaemonError::Xrpc("JWT lifetime is invalid".into()));
    }
    let parts: Vec<_> = jwt.split('.').collect();
    let signature = URL_SAFE_NO_PAD
        .decode(parts[2])
        .map_err(|_| DaemonError::Xrpc("malformed JWT signature".into()))?;
    let digest = Sha256::digest(format!("{}.{}", parts[0], parts[1]).as_bytes());
    let did_key = state.resolver.signing_key(&claims.iss).await?;
    let valid = rsky_crypto::verify::verify_signature_digest(&did_key, &digest, &signature, None)
        .map_err(|e| DaemonError::Xrpc(e.to_string()))?;
    valid
        .then_some(())
        .ok_or_else(|| DaemonError::Xrpc("bad JWT signature".into()))
}

async fn intake(
    State(state): State<IntakeState>,
    headers: HeaderMap,
    Json(mut request): Json<IntakeRequest>,
) -> (StatusCode, Json<Value>) {
    let parsed = match rsky_space::space_id::SpaceId::parse(&request.space) {
        Ok(value) => value,
        Err(error) => return (StatusCode::BAD_REQUEST, error_body("InvalidRequest", error)),
    };
    let canonical = parsed.uri();
    if canonical != request.space
        || parsed.space_type != state.space_type
        || request.contract_version != 1
        || request.generation <= 0
        || !matches!(request.desired_state.as_str(), "track" | "untrack")
        || (request.desired_state == "track"
            && !matches!(
                request.lifecycle_state.as_str(),
                "host_registered" | "active" | "deleting"
            ))
        || (request.desired_state == "untrack" && request.lifecycle_state != "inactive")
    {
        return (
            StatusCode::BAD_REQUEST,
            error_body("InvalidRequest", "invalid intake contract"),
        );
    }
    request.space = canonical.clone();
    let expected_event = event_id(
        &canonical,
        request.generation,
        &request.desired_state,
        &request.lifecycle_state,
    );
    if request.event_id != expected_event
        || request.event_id.len() != 64
        || !request
            .event_id
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return (
            StatusCode::BAD_REQUEST,
            error_body("InvalidRequest", "eventId does not match request"),
        );
    }
    if let Err(error) = authenticate(&headers, &state, &request).await {
        return (
            StatusCode::UNAUTHORIZED,
            error_body("AuthenticationRequired", error),
        );
    }
    let hash = payload_hash(&request);
    let db = Arc::clone(&state.db);
    let direct = crate::sqlite_index::DirectIntake {
        space_uri: canonical.clone(),
        generation: request.generation,
        desired_state: request.desired_state.clone(),
        lifecycle_state: request.lifecycle_state.clone(),
        event_id: request.event_id.clone(),
        payload_hash: hash,
        observed_at: (state.now_fn)() as i64,
    };
    let outcome = match tokio::task::spawn_blocking(move || db.apply_direct(direct)).await {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                error_body("StoreUnavailable", error),
            )
        }
        Err(error) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                error_body("StoreUnavailable", DaemonError::Index(error.to_string())),
            )
        }
    };
    let status = match outcome {
        DirectIntakeOutcome::Accepted => StatusCode::ACCEPTED,
        DirectIntakeOutcome::Duplicate => StatusCode::OK,
        DirectIntakeOutcome::Stale { stored_generation } => {
            return (
                StatusCode::CONFLICT,
                Json(json!({"error":"StaleGeneration","storedGeneration":stored_generation})),
            )
        }
        DirectIntakeOutcome::Conflict => {
            return (
                StatusCode::CONFLICT,
                error_body("EventConflict", "event conflicts with stored state"),
            )
        }
    };
    if state
        .supervisor
        .send(if request.desired_state == "track" {
            SupervisorCommand::Track {
                space: canonical.clone(),
                generation: request.generation,
                state: request.lifecycle_state.clone(),
            }
        } else {
            SupervisorCommand::Untrack {
                space: canonical.clone(),
                generation: request.generation,
            }
        })
        .await
        .is_err()
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            error_body("ShuttingDown", "supervisor is unavailable"),
        );
    }
    (
        status,
        Json(
            serde_json::to_value(IntakeResponse {
                contract_version: 1,
                event_id: request.event_id,
                space: canonical,
                generation: request.generation,
                desired_state: request.desired_state,
                result: if matches!(outcome, DirectIntakeOutcome::Duplicate) {
                    "duplicate"
                } else {
                    "accepted"
                },
                persisted: true,
            })
            .unwrap(),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::CommitKeyResolver;
    use async_trait::async_trait;
    use axum::body::Body;
    use axum::http::Request;
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use tower::ServiceExt;

    struct FixedKey;

    #[async_trait]
    impl CommitKeyResolver for FixedKey {
        async fn signing_key(&self, _did: &str) -> Result<String> {
            Ok("did:key:zExample".to_string())
        }
    }

    #[test]
    fn event_id_is_cross_language_stable() {
        assert_eq!(
            event_id(
                "at://did:plc:authority/space/community.blacksky.feed/main",
                7,
                "track",
                "host_registered"
            ),
            "3c8596e9e02e650635d6294195181030b01ffd644b2db0ec22504e243648da0d"
        );
    }

    #[test]
    fn unknown_fields_and_invalid_state_pairs_are_rejected_by_the_contract_shape() {
        let unknown = serde_json::from_value::<IntakeRequest>(serde_json::json!({
            "contractVersion": 1,
            "eventId": "a",
            "space": "at://did:plc:a/space/community.blacksky.feed/main",
            "generation": 1,
            "desiredState": "track",
            "lifecycleState": "active",
            "unexpected": true
        }));
        assert!(unknown.is_err());
        assert_ne!(
            event_id(
                "at://did:plc:a/space/community.blacksky.feed/main",
                1,
                "track",
                "host_registered"
            ),
            event_id(
                "at://did:plc:a/space/community.blacksky.feed/main",
                1,
                "untrack",
                "inactive"
            )
        );
    }

    #[tokio::test]
    async fn intake_http_rejects_missing_authentication() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("daemon.sqlite");
        let db = Arc::new(SqliteIndex::open(db_path.to_str().unwrap()).unwrap());
        let (supervisor, _commands) = mpsc::channel(1);
        let app = router(IntakeState {
            db,
            supervisor,
            issuer_did: "did:plc:feedgen".into(),
            service_identity: "did:web:daemon.example".into(),
            space_type: "community.blacksky.feed".into(),
            resolver: Arc::new(FixedKey),
            now_fn: || 1_000_000,
        });
        let body = serde_json::to_vec(&IntakeRequest {
            contract_version: 1,
            event_id: event_id(
                "at://did:plc:authority/space/community.blacksky.feed/main",
                7,
                "track",
                "active",
            ),
            space: "at://did:plc:authority/space/community.blacksky.feed/main".into(),
            generation: 7,
            desired_state: "track".into(),
            lifecycle_state: "active".into(),
        })
        .unwrap();
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/internal/v1/spaces/intake")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
