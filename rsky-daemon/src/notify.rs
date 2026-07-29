//! Inbound write notifications (proposal §Write notifications, §Space
//! deletion): the space host POSTs `notifyWrite` when a member's repo advances
//! and `notifySpaceDeleted` when the space is torn down, authenticated with a
//! service-auth JWT signed by the space host / authority DID.

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rsky_lexicon::com::atproto::space::{NotifySpaceDeletedInput, NotifyWriteInput};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::Digest;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::engine::CommitKeyResolver;
use crate::error::{DaemonError, Result};
use crate::index::SpaceIndex;

/// A queued `(space, did)` write notice for the runner to pull.
pub type WriteNotice = (String, String);

#[derive(Debug, Deserialize)]
pub struct ServiceAuthClaims {
    pub iss: String,
    pub aud: String,
    pub exp: u64,
}

/// Verify a service-auth bearer JWT from the space host: `iss` is the
/// authority, `aud` is this syncer's service identity, unexpired, and signed
/// by the authority's resolved key (over `sha256(header_b64.payload_b64)` per
/// the atproto inter-service-auth convention).
pub fn verify_service_auth(
    jwt: &str,
    expected_iss: &str,
    expected_aud: &str,
    did_key: &str,
    now: u64,
) -> Result<()> {
    let claims = decode_claims(jwt)?;
    if claims.iss != expected_iss {
        return Err(rsky_space::SpaceError::InvalidClaim("iss != space host".into()).into());
    }
    if claims.aud != expected_aud {
        return Err(rsky_space::SpaceError::InvalidClaim("aud != this syncer".into()).into());
    }
    if now >= claims.exp {
        return Err(rsky_space::SpaceError::Expired.into());
    }
    let parts: Vec<&str> = jwt.split('.').collect();
    let signature = URL_SAFE_NO_PAD
        .decode(parts[2])
        .map_err(|e| rsky_space::SpaceError::MalformedJwt(e.to_string()))?;
    let signing_input = format!("{}.{}", parts[0], parts[1]);
    let digest = sha2::Sha256::digest(signing_input.as_bytes());
    let ok = rsky_crypto::verify::verify_signature_digest(
        &did_key.to_string(),
        &digest,
        &signature,
        None,
    )
    .map_err(|e| rsky_space::SpaceError::Crypto(e.to_string()))?;
    if ok {
        Ok(())
    } else {
        Err(rsky_space::SpaceError::BadSignature.into())
    }
}

/// Decode a service-auth JWT's claims without verifying its signature.
pub fn decode_claims(jwt: &str) -> Result<ServiceAuthClaims> {
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() != 3 {
        return Err(rsky_space::SpaceError::MalformedJwt("expected 3 segments".into()).into());
    }
    let payload = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|e| rsky_space::SpaceError::MalformedJwt(e.to_string()))?;
    let claims: ServiceAuthClaims = serde_json::from_slice(&payload)
        .map_err(|e| rsky_space::SpaceError::MalformedJwt(e.to_string()))?;
    Ok(claims)
}

#[derive(Clone)]
pub struct NotifyState {
    pub space_uri: String,
    /// The authority (space host) DID whose key signs inbound notifications.
    pub authority_did: String,
    /// This syncer's service identity: the required `aud` on inbound tokens.
    pub service_identity: String,
    pub resolver: Arc<dyn CommitKeyResolver>,
    pub index: Arc<dyn SpaceIndex>,
    pub tx: mpsc::Sender<WriteNotice>,
    pub now_fn: fn() -> u64,
}

pub fn router(state: NotifyState) -> Router {
    Router::new()
        .route("/xrpc/com.atproto.space.notifyWrite", post(notify_write))
        .route(
            "/xrpc/com.atproto.space.notifySpaceDeleted",
            post(notify_space_deleted),
        )
        .with_state(state)
}

fn error_body(error: &str, message: impl std::fmt::Display) -> Json<Value> {
    Json(json!({ "error": error, "message": message.to_string() }))
}

async fn authenticate(headers: &HeaderMap, state: &NotifyState) -> Result<()> {
    let jwt = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| {
            DaemonError::Space(rsky_space::SpaceError::MalformedJwt(
                "missing bearer token".into(),
            ))
        })?;
    let did_key = state.resolver.signing_key(&state.authority_did).await?;
    verify_service_auth(
        jwt,
        &state.authority_did,
        &state.service_identity,
        &did_key,
        (state.now_fn)(),
    )
}

async fn notify_write(
    State(state): State<NotifyState>,
    headers: HeaderMap,
    Json(input): Json<NotifyWriteInput>,
) -> (StatusCode, Json<Value>) {
    if let Err(e) = authenticate(&headers, &state).await {
        return (
            StatusCode::UNAUTHORIZED,
            error_body("AuthenticationRequired", e),
        );
    }
    if input.space != state.space_uri {
        return (
            StatusCode::BAD_REQUEST,
            error_body("InvalidRequest", "space is not synced by this daemon"),
        );
    }
    tracing::debug!(space = %input.space, did = %input.did, rev = %input.rev, "write notice");
    if state.tx.send((input.space, input.did)).await.is_err() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            error_body("ShuttingDown", "notify queue closed"),
        );
    }
    (StatusCode::OK, Json(json!({})))
}

async fn notify_space_deleted(
    State(state): State<NotifyState>,
    headers: HeaderMap,
    Json(input): Json<NotifySpaceDeletedInput>,
) -> (StatusCode, Json<Value>) {
    if let Err(e) = authenticate(&headers, &state).await {
        return (
            StatusCode::UNAUTHORIZED,
            error_body("AuthenticationRequired", e),
        );
    }
    if input.space != state.space_uri {
        return (
            StatusCode::BAD_REQUEST,
            error_body("InvalidRequest", "space is not synced by this daemon"),
        );
    }
    tracing::warn!(space = %input.space, "space deleted; purging all synced data");
    if let Err(e) = state.index.purge_space().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            error_body("InternalServerError", e),
        );
    }
    (StatusCode::OK, Json(json!({})))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::InMemoryIndex;
    use async_trait::async_trait;
    use axum::body::Body;
    use axum::http::Request;
    use secp256k1::{Message, PublicKey, Secp256k1, SecretKey};
    use sha2::Sha256;
    use tower::ServiceExt;

    const SPACE: &str = "at://did:plc:authority/space/community.blacksky.feed/main";
    const AUTHORITY: &str = "did:plc:authority";
    const SYNCER: &str = "did:web:syncer.blacksky.community";
    const NOW: u64 = 1_000_000;

    fn host_key() -> (SecretKey, String) {
        let secret = SecretKey::from_slice(&[0x44u8; 32]).unwrap();
        let pubkey = PublicKey::from_secret_key(&Secp256k1::new(), &secret);
        (secret, rsky_crypto::utils::encode_did_key(&pubkey))
    }

    fn service_jwt(secret: &SecretKey, iss: &str, aud: &str, exp: u64) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"typ":"JWT","alg":"ES256K"}"#);
        let payload =
            URL_SAFE_NO_PAD.encode(json!({ "iss": iss, "aud": aud, "exp": exp }).to_string());
        let input = format!("{header}.{payload}");
        let digest = Sha256::digest(input.as_bytes());
        let msg = Message::from_digest_slice(&digest).unwrap();
        let mut sig = secret.sign_ecdsa(msg);
        sig.normalize_s();
        format!(
            "{input}.{}",
            URL_SAFE_NO_PAD.encode(sig.serialize_compact())
        )
    }

    struct FixedKey(String);
    #[async_trait]
    impl CommitKeyResolver for FixedKey {
        async fn signing_key(&self, did: &str) -> Result<String> {
            assert_eq!(did, AUTHORITY);
            Ok(self.0.clone())
        }
    }

    fn fixed_now() -> u64 {
        NOW
    }

    fn state(
        did_key: &str,
        index: Arc<dyn SpaceIndex>,
        tx: mpsc::Sender<WriteNotice>,
    ) -> NotifyState {
        NotifyState {
            space_uri: SPACE.to_string(),
            authority_did: AUTHORITY.to_string(),
            service_identity: SYNCER.to_string(),
            resolver: Arc::new(FixedKey(did_key.to_string())),
            index,
            tx,
            now_fn: fixed_now,
        }
    }

    fn request(path: &str, token: Option<&str>, body: Value) -> Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json");
        if let Some(token) = token {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    fn write_body(did: &str) -> Value {
        json!({ "space": SPACE, "did": did, "rev": "3krev" })
    }

    #[tokio::test]
    async fn valid_notify_write_queues_the_repo() {
        let (secret, did_key) = host_key();
        let (tx, mut rx) = mpsc::channel(4);
        let app = router(state(&did_key, Arc::new(InMemoryIndex::new()), tx));
        let jwt = service_jwt(&secret, AUTHORITY, SYNCER, NOW + 60);

        let resp = app
            .oneshot(request(
                "/xrpc/com.atproto.space.notifyWrite",
                Some(&jwt),
                write_body("did:plc:writer"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            rx.recv().await.unwrap(),
            (SPACE.to_string(), "did:plc:writer".to_string())
        );
    }

    #[tokio::test]
    async fn notify_write_auth_failures_are_401() {
        let (secret, did_key) = host_key();
        let (wrong_secret, _) = {
            let secret = SecretKey::from_slice(&[0x55u8; 32]).unwrap();
            let pubkey = PublicKey::from_secret_key(&Secp256k1::new(), &secret);
            (secret, rsky_crypto::utils::encode_did_key(&pubkey))
        };
        let (tx, _rx) = mpsc::channel(4);
        let app = router(state(&did_key, Arc::new(InMemoryIndex::new()), tx));

        for token in [
            None,
            Some("not.a.jwt!".to_string()),
            Some(service_jwt(&secret, AUTHORITY, SYNCER, NOW)), // expired (now >= exp)
            Some(service_jwt(&secret, AUTHORITY, "did:web:other", NOW + 60)), // wrong aud
            Some(service_jwt(&secret, "did:plc:imposter", SYNCER, NOW + 60)), // wrong iss
            Some(service_jwt(&wrong_secret, AUTHORITY, SYNCER, NOW + 60)), // wrong key
        ] {
            let resp = app
                .clone()
                .oneshot(request(
                    "/xrpc/com.atproto.space.notifyWrite",
                    token.as_deref(),
                    write_body("did:plc:writer"),
                ))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }
    }

    #[tokio::test]
    async fn notify_write_for_unknown_space_is_400() {
        let (secret, did_key) = host_key();
        let (tx, _rx) = mpsc::channel(4);
        let app = router(state(&did_key, Arc::new(InMemoryIndex::new()), tx));
        let jwt = service_jwt(&secret, AUTHORITY, SYNCER, NOW + 60);

        let resp = app
            .oneshot(request(
                "/xrpc/com.atproto.space.notifyWrite",
                Some(&jwt),
                json!({ "space": "at://x/space/y/z", "did": "did:plc:w", "rev": "3k" }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn notify_write_with_closed_queue_is_503() {
        let (secret, did_key) = host_key();
        let (tx, rx) = mpsc::channel(4);
        drop(rx);
        let app = router(state(&did_key, Arc::new(InMemoryIndex::new()), tx));
        let jwt = service_jwt(&secret, AUTHORITY, SYNCER, NOW + 60);

        let resp = app
            .oneshot(request(
                "/xrpc/com.atproto.space.notifyWrite",
                Some(&jwt),
                write_body("did:plc:writer"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn notify_space_deleted_purges_the_index() {
        let (secret, did_key) = host_key();
        let index = Arc::new(InMemoryIndex::new());
        index
            .upsert("did:plc:w", "c.o.l", "3ka", "bafyA", "3rev", None)
            .await
            .unwrap();
        let (tx, _rx) = mpsc::channel(4);
        let app = router(state(&did_key, index.clone(), tx));
        let jwt = service_jwt(&secret, AUTHORITY, SYNCER, NOW + 60);

        let resp = app
            .oneshot(request(
                "/xrpc/com.atproto.space.notifySpaceDeleted",
                Some(&jwt),
                json!({ "space": SPACE }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(index.record_count("did:plc:w"), 0);
    }

    #[tokio::test]
    async fn notify_space_deleted_rejects_bad_auth_and_wrong_space() {
        let (secret, did_key) = host_key();
        let (tx, _rx) = mpsc::channel(4);
        let app = router(state(&did_key, Arc::new(InMemoryIndex::new()), tx));

        let resp = app
            .clone()
            .oneshot(request(
                "/xrpc/com.atproto.space.notifySpaceDeleted",
                None,
                json!({ "space": SPACE }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let jwt = service_jwt(&secret, AUTHORITY, SYNCER, NOW + 60);
        let resp = app
            .oneshot(request(
                "/xrpc/com.atproto.space.notifySpaceDeleted",
                Some(&jwt),
                json!({ "space": "at://x/space/y/z" }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn notify_space_deleted_surfaces_purge_failure() {
        struct FailingIndex;
        #[async_trait]
        impl SpaceIndex for FailingIndex {
            async fn last_rev(&self, _did: &str) -> Result<Option<String>> {
                Ok(None)
            }
            async fn load_lthash(&self, _did: &str) -> Result<rsky_space::LtHash> {
                Ok(rsky_space::LtHash::new())
            }
            async fn get_cid(
                &self,
                _did: &str,
                _collection: &str,
                _rkey: &str,
            ) -> Result<Option<String>> {
                Ok(None)
            }
            async fn upsert(
                &self,
                _did: &str,
                _collection: &str,
                _rkey: &str,
                _cid: &str,
                _rev: &str,
                _value: Option<Vec<u8>>,
            ) -> Result<()> {
                Ok(())
            }
            async fn delete(&self, _did: &str, _collection: &str, _rkey: &str) -> Result<()> {
                Ok(())
            }
            async fn save_head(
                &self,
                _did: &str,
                _rev: &str,
                _lthash: &rsky_space::LtHash,
            ) -> Result<()> {
                Ok(())
            }
            async fn list_paths(&self, _did: &str) -> Result<Vec<(String, String, String)>> {
                Ok(vec![])
            }
            async fn purge_space(&self) -> Result<()> {
                Err(DaemonError::Index("disk full".to_string()))
            }
        }

        let (secret, did_key) = host_key();
        let (tx, _rx) = mpsc::channel(4);
        let app = router(state(&did_key, Arc::new(FailingIndex), tx));
        let jwt = service_jwt(&secret, AUTHORITY, SYNCER, NOW + 60);

        let resp = app
            .oneshot(request(
                "/xrpc/com.atproto.space.notifySpaceDeleted",
                Some(&jwt),
                json!({ "space": SPACE }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);

        // Exercise FailingIndex's trivial passthroughs for completeness.
        let idx = FailingIndex;
        idx.last_rev("d").await.unwrap();
        idx.load_lthash("d").await.unwrap();
        idx.get_cid("d", "c", "r").await.unwrap();
        idx.upsert("d", "c", "r", "cid", "rev", None).await.unwrap();
        idx.delete("d", "c", "r").await.unwrap();
        idx.save_head("d", "rev", &rsky_space::LtHash::new())
            .await
            .unwrap();
        idx.list_paths("d").await.unwrap();
    }

    #[test]
    fn decode_claims_rejects_malformed_tokens() {
        assert!(decode_claims("one.two").is_err());
        assert!(decode_claims("!!.@@.##").is_err());
        let not_claims = format!(
            "{}.{}.{}",
            URL_SAFE_NO_PAD.encode(b"{}"),
            URL_SAFE_NO_PAD.encode(b"[1,2]"),
            URL_SAFE_NO_PAD.encode(b"sig")
        );
        assert!(decode_claims(&not_claims).is_err());
    }

    #[test]
    fn verify_service_auth_rejects_undecodable_signature() {
        let (_, did_key) = host_key();
        let header = URL_SAFE_NO_PAD.encode(br#"{"typ":"JWT","alg":"ES256K"}"#);
        let payload = URL_SAFE_NO_PAD
            .encode(json!({ "iss": AUTHORITY, "aud": SYNCER, "exp": NOW + 60 }).to_string());
        let jwt = format!("{header}.{payload}.!!!");
        assert!(matches!(
            verify_service_auth(&jwt, AUTHORITY, SYNCER, &did_key, NOW),
            Err(DaemonError::Space(rsky_space::SpaceError::MalformedJwt(_)))
        ));
    }

    #[test]
    fn verify_service_auth_surfaces_crypto_errors() {
        let (secret, _) = host_key();
        let jwt = service_jwt(&secret, AUTHORITY, SYNCER, NOW + 60);
        assert!(matches!(
            verify_service_auth(&jwt, AUTHORITY, SYNCER, "did:key:zBogus", NOW),
            Err(DaemonError::Space(rsky_space::SpaceError::Crypto(_)))
        ));
    }
}
