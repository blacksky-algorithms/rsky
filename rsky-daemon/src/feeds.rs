//! Projection to the feed service's record ingress.

use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::debug;

use crate::error::{DaemonError, Result};
use crate::projection::Projector;
use crate::router::{SyncEvent, POST_COLLECTION};
use crate::service_jwt::ServiceJwtIssuer;
use crate::unix_now;

pub const PROJECT_RECORDS_LXM: &str = "community.blacksky.space.projectRecords";
pub const ACK_SYNCERS_OBSERVED_LXM: &str = "community.blacksky.space.ackSyncersObserved";
/// The receiving side rejects a token whose lifetime exceeds five minutes;
/// the margin absorbs clock skew between the two services.
const TOKEN_TTL_SECS: u64 = 240;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ProjectRecordsRequest {
    pub ops: Vec<ProjectRecord>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProjectionOperation {
    Create,
    Delete,
    Flag,
    Unflag,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ProjectRecord {
    pub space: String,
    pub author: String,
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,
    pub revision: String,
    pub operation: ProjectionOperation,
    pub collection: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record: Option<Value>,
    #[serde(rename = "actionUri", skip_serializing_if = "Option::is_none")]
    pub action_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub neg: Option<bool>,
}

#[async_trait]
pub trait ProjectionIngress: Send + Sync {
    async fn project_records(&self, request: &ProjectRecordsRequest) -> Result<()>;
}

#[async_trait]
impl<T: ProjectionIngress> ProjectionIngress for std::sync::Arc<T> {
    async fn project_records(&self, request: &ProjectRecordsRequest) -> Result<()> {
        self.as_ref().project_records(request).await
    }
}

/// Tells the feed service this space now has a syncer keeping it current.
/// Until it hears that, it refuses the space's projections.
#[async_trait]
pub trait SpaceLifecycleAcker: Send + Sync {
    async fn acknowledge_sync(&self, space: &str, generation: i64) -> Result<()>;
}

/// Posts batches to a service that accepts `projectRecords`, authenticated
/// with this daemon's own service identity.
pub struct HttpProjectionIngress {
    target: &'static str,
    base_url: String,
    audience: String,
    issuer: ServiceJwtIssuer,
    http: reqwest::Client,
}

impl HttpProjectionIngress {
    pub fn new(
        target: &'static str,
        base_url: impl Into<String>,
        service_identity: impl Into<String>,
        audience: impl Into<String>,
        signing_key_hex: &str,
    ) -> Result<Self> {
        Ok(Self {
            target,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            audience: audience.into(),
            issuer: ServiceJwtIssuer::from_hex(service_identity, signing_key_hex)?,
            http: reqwest::Client::new(),
        })
    }

    fn service_jwt(&self, lxm: &str) -> Result<String> {
        static JTI: AtomicU64 = AtomicU64::new(1);
        let now = unix_now();
        let jti = format!("{now}-{}", JTI.fetch_add(1, Ordering::Relaxed));
        self.issuer
            .mint_for(&self.audience, lxm, now, TOKEN_TTL_SECS, &jti)
    }
}

#[async_trait]
impl SpaceLifecycleAcker for HttpProjectionIngress {
    async fn acknowledge_sync(&self, space: &str, generation: i64) -> Result<()> {
        let response = self
            .http
            .post(format!("{}/xrpc/{ACK_SYNCERS_OBSERVED_LXM}", self.base_url))
            .bearer_auth(self.service_jwt(ACK_SYNCERS_OBSERVED_LXM)?)
            .json(&serde_json::json!({"space": space, "generation": generation}))
            .send()
            .await
            .map_err(|error| DaemonError::Xrpc(error.to_string()))?;
        if !response.status().is_success() {
            return Err(DaemonError::Xrpc(format!(
                "{} {ACK_SYNCERS_OBSERVED_LXM} returned {}",
                self.target,
                response.status()
            )));
        }
        Ok(())
    }
}

#[async_trait]
impl ProjectionIngress for HttpProjectionIngress {
    async fn project_records(&self, request: &ProjectRecordsRequest) -> Result<()> {
        let response = self
            .http
            .post(format!("{}/xrpc/{PROJECT_RECORDS_LXM}", self.base_url))
            .bearer_auth(self.service_jwt(PROJECT_RECORDS_LXM)?)
            .json(request)
            .send()
            .await
            .map_err(|error| {
                DaemonError::RetryableProjection(format!("{} unreachable: {error}", self.target))
            })?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        let message = format!("{} {PROJECT_RECORDS_LXM} returned {status}", self.target);
        if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(DaemonError::RetryableProjection(message));
        }
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(DaemonError::AdmissionDenied(message));
        }
        Err(DaemonError::Xrpc(message))
    }
}

/// Projects a space's posts and their moderation state to the feed service.
pub struct FeedsProjector<I: ProjectionIngress> {
    ingress: I,
    space: String,
}

impl<I: ProjectionIngress> FeedsProjector<I> {
    pub fn new(ingress: I, space: impl Into<String>) -> Self {
        Self {
            ingress,
            space: space.into(),
        }
    }
}

#[async_trait]
impl<I: ProjectionIngress> Projector for FeedsProjector<I> {
    fn name(&self) -> &'static str {
        "feeds"
    }

    async fn project(&self, author: &str, revision: &str, events: &[SyncEvent]) -> Result<()> {
        // One URI may change more than once in a batch; only its final state
        // is worth sending.
        let mut final_posts: BTreeMap<&str, &SyncEvent> = BTreeMap::new();
        let mut ops = Vec::new();
        for event in events {
            match event {
                SyncEvent::PostCreated { .. } | SyncEvent::PostDeleted { .. } => {
                    final_posts.insert(event.uri(), event);
                }
                SyncEvent::ModerationAction { uri, record, .. } => {
                    if let Some(op) = self.moderation_op(author, revision, uri, record.as_ref()) {
                        ops.push(op);
                    }
                }
                other => debug!(uri = other.uri(), "feeds projection ignores this event"),
            }
        }
        for event in final_posts.into_values() {
            match event {
                SyncEvent::PostCreated {
                    uri, cid, record, ..
                } => ops.push(ProjectRecord {
                    space: self.space.clone(),
                    author: author.to_string(),
                    uri: uri.clone(),
                    cid: Some(cid.clone()),
                    revision: revision.to_string(),
                    operation: ProjectionOperation::Create,
                    collection: POST_COLLECTION.to_string(),
                    record: Some(record.clone()),
                    action_uri: None,
                    neg: None,
                }),
                SyncEvent::PostDeleted { uri, .. } => ops.push(ProjectRecord {
                    space: self.space.clone(),
                    author: author.to_string(),
                    uri: uri.clone(),
                    cid: None,
                    revision: revision.to_string(),
                    operation: ProjectionOperation::Delete,
                    collection: POST_COLLECTION.to_string(),
                    record: None,
                    action_uri: None,
                    neg: None,
                }),
                _ => unreachable!("only post events were retained"),
            }
        }
        if ops.is_empty() {
            return Ok(());
        }
        self.ingress
            .project_records(&ProjectRecordsRequest { ops })
            .await
    }
}

impl<I: ProjectionIngress> FeedsProjector<I> {
    fn moderation_op(
        &self,
        author: &str,
        revision: &str,
        uri: &str,
        record: Option<&Value>,
    ) -> Option<ProjectRecord> {
        let record = record?;
        if record["val"].as_str() != Some("remove") {
            return None;
        }
        // A lift is its own signed record carrying the action it reverses,
        // so it names that action's URI rather than its own.
        let neg = record["neg"].as_bool() == Some(true);
        let (action_uri, cid) = if neg {
            (record["action"]["uri"].as_str(), None)
        } else {
            (Some(uri), record["subject"]["cid"].as_str())
        };
        let Some(action_uri) = action_uri else {
            debug!(%uri, "moderation action has no action URI");
            return None;
        };
        if !neg && cid.is_none() {
            debug!(%uri, "moderation action has no strong post reference");
            return None;
        }
        Some(ProjectRecord {
            space: self.space.clone(),
            author: author.to_string(),
            uri: uri.to_string(),
            cid: cid.map(str::to_string),
            revision: revision.to_string(),
            operation: if neg {
                ProjectionOperation::Unflag
            } else {
                ProjectionOperation::Flag
            },
            collection: POST_COLLECTION.to_string(),
            record: None,
            action_uri: Some(action_uri.to_string()),
            neg: Some(neg),
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    pub(crate) const SPACE: &str = "at://did:plc:community/space/community.blacksky.feed/private";
    pub(crate) const AUTHOR: &str = "did:plc:alice";

    #[derive(Default)]
    pub(crate) struct CapturingIngress {
        pub sent: Mutex<Vec<ProjectRecordsRequest>>,
    }

    #[async_trait]
    impl ProjectionIngress for CapturingIngress {
        async fn project_records(&self, request: &ProjectRecordsRequest) -> Result<()> {
            self.sent.lock().unwrap().push(request.clone());
            Ok(())
        }
    }

    pub(crate) fn post_created(rkey: &str, text: &str) -> SyncEvent {
        SyncEvent::PostCreated {
            uri: format!("{SPACE}/{AUTHOR}/{POST_COLLECTION}/{rkey}"),
            author: AUTHOR.to_string(),
            cid: "bafypost".to_string(),
            rev: "3krev".to_string(),
            record: json!({"$type": POST_COLLECTION, "text": text, "createdAt": "2026-08-19T00:00:00Z"}),
        }
    }

    fn ingress(server: &MockServer) -> HttpProjectionIngress {
        HttpProjectionIngress::new(
            "feeds",
            server.uri(),
            "did:web:syncer.example",
            "did:web:feeds.example",
            &hex::encode([1_u8; 32]),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn a_batch_projects_final_post_state_once() {
        let projector = FeedsProjector::new(CapturingIngress::default(), SPACE);
        let deleted = SyncEvent::PostDeleted {
            uri: format!("{SPACE}/{AUTHOR}/{POST_COLLECTION}/3ka"),
            author: AUTHOR.to_string(),
        };
        projector
            .project(
                AUTHOR,
                "3krev",
                &[
                    post_created("3ka", "first"),
                    deleted,
                    post_created("3kb", "second"),
                ],
            )
            .await
            .unwrap();

        let sent = projector.ingress.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        let ops = &sent[0].ops;
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0].operation, ProjectionOperation::Delete);
        assert_eq!(
            ops[0].uri,
            format!("{SPACE}/{AUTHOR}/{POST_COLLECTION}/3ka")
        );
        assert_eq!(ops[1].operation, ProjectionOperation::Create);
        assert_eq!(ops[1].record.as_ref().unwrap()["text"], "second");
        assert_eq!(ops[1].space, SPACE);
        assert_eq!(ops[1].revision, "3krev");
    }

    #[tokio::test]
    async fn moderation_removals_and_lifts_become_flags() {
        let projector = FeedsProjector::new(CapturingIngress::default(), SPACE);
        let action_uri =
            format!("{SPACE}/did:plc:community/community.blacksky.moderation.action/3m");
        let events = vec![
            SyncEvent::ModerationAction {
                uri: action_uri.clone(),
                rev: "3krev".to_string(),
                record: Some(json!({
                    "val": "remove",
                    "subject": {"uri": format!("{SPACE}/{AUTHOR}/{POST_COLLECTION}/3ka"), "cid": "bafypost"},
                })),
            },
            SyncEvent::ModerationAction {
                uri: format!("{SPACE}/did:plc:community/community.blacksky.moderation.action/3n"),
                rev: "3krev".to_string(),
                record: Some(json!({
                    "val": "remove",
                    "neg": true,
                    "action": {"uri": action_uri},
                })),
            },
            // Neither a removal nor decodable as one: dropped, not sent.
            SyncEvent::ModerationAction {
                uri: format!("{SPACE}/did:plc:community/community.blacksky.moderation.action/3o"),
                rev: "3krev".to_string(),
                record: Some(json!({"val": "spam"})),
            },
        ];
        projector.project(AUTHOR, "3krev", &events).await.unwrap();

        let sent = projector.ingress.sent.lock().unwrap();
        let ops = &sent[0].ops;
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0].operation, ProjectionOperation::Flag);
        assert_eq!(ops[0].neg, Some(false));
        assert_eq!(ops[0].cid.as_deref(), Some("bafypost"));
        assert_eq!(ops[1].operation, ProjectionOperation::Unflag);
        assert_eq!(ops[1].neg, Some(true));
        assert_eq!(ops[1].action_uri.as_deref(), Some(action_uri.as_str()));
    }

    #[tokio::test]
    async fn a_batch_with_nothing_to_send_makes_no_request() {
        let projector = FeedsProjector::new(CapturingIngress::default(), SPACE);
        projector
            .project(
                AUTHOR,
                "3krev",
                &[SyncEvent::LikeDeleted {
                    uri: format!("{SPACE}/{AUTHOR}/app.bsky.feed.like/3ka"),
                    author: AUTHOR.to_string(),
                }],
            )
            .await
            .unwrap();
        assert!(projector.ingress.sent.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_batch_reaches_the_ingress_with_a_method_bound_service_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(format!("/xrpc/{PROJECT_RECORDS_LXM}")))
            .and(wiremock::matchers::header_regex(
                "authorization",
                "^Bearer ",
            ))
            .and(wiremock::matchers::body_string_contains("\"create\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;

        let projector = FeedsProjector::new(ingress(&server), SPACE);
        projector
            .project(AUTHOR, "3krev", &[post_created("3ka", "hello")])
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn destination_outages_are_retryable_and_rejections_are_not() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let error = ingress(&server)
            .project_records(&ProjectRecordsRequest { ops: vec![] })
            .await
            .unwrap_err();
        assert!(error.is_retryable_projection());

        let rejecting = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(400))
            .mount(&rejecting)
            .await;
        let error = ingress(&rejecting)
            .project_records(&ProjectRecordsRequest { ops: vec![] })
            .await
            .unwrap_err();
        assert!(!error.is_retryable_projection());
        assert!(!error.is_admission_denied());

        let denying = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_string(
                r#"{"error":"NotAuthorized","message":"author is not admitted to this space"}"#,
            ))
            .mount(&denying)
            .await;
        let error = ingress(&denying)
            .project_records(&ProjectRecordsRequest { ops: vec![] })
            .await
            .unwrap_err();
        assert!(error.is_admission_denied());
        assert!(!error.is_retryable_projection());

        let unreachable = HttpProjectionIngress::new(
            "feeds",
            "http://127.0.0.1:1",
            "did:web:syncer.example",
            "did:web:feeds.example",
            &hex::encode([1_u8; 32]),
        )
        .unwrap();
        assert!(unreachable
            .project_records(&ProjectRecordsRequest { ops: vec![] })
            .await
            .unwrap_err()
            .is_retryable_projection());
    }
}
