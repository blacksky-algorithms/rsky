//! Projection to the appview's record ingress.

use async_trait::async_trait;
use serde_json::Value;
use std::collections::BTreeMap;

use crate::error::Result;
use crate::feeds::{ProjectRecord, ProjectRecordsRequest, ProjectionIngress, ProjectionOperation};
use crate::projection::Projector;
use crate::router::{SyncEvent, LIKE_COLLECTION, MODERATION_ACTION_COLLECTION, POST_COLLECTION};

/// Projects a space's posts, likes and moderation state to the appview.
pub struct AppviewProjector<I: ProjectionIngress> {
    ingress: I,
    space: String,
}

impl<I: ProjectionIngress> AppviewProjector<I> {
    pub fn new(ingress: I, space: impl Into<String>) -> Self {
        Self {
            ingress,
            space: space.into(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn op(
        &self,
        author: &str,
        uri: &str,
        cid: Option<&str>,
        revision: &str,
        operation: ProjectionOperation,
        collection: &str,
        record: Option<Value>,
    ) -> ProjectRecord {
        ProjectRecord {
            space: self.space.clone(),
            author: author.to_string(),
            uri: uri.to_string(),
            cid: cid.map(str::to_string),
            revision: revision.to_string(),
            operation,
            collection: collection.to_string(),
            record,
            action_uri: None,
            neg: None,
        }
    }

    fn moderation_op(
        &self,
        author: &str,
        revision: &str,
        uri: &str,
        record: &Value,
    ) -> Option<ProjectRecord> {
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
        Some(ProjectRecord {
            action_uri: Some(action_uri?.to_string()),
            neg: Some(neg),
            ..self.op(
                author,
                uri,
                cid,
                revision,
                if neg {
                    ProjectionOperation::Unflag
                } else {
                    ProjectionOperation::Flag
                },
                MODERATION_ACTION_COLLECTION,
                None,
            )
        })
    }
}

#[async_trait]
impl<I: ProjectionIngress> Projector for AppviewProjector<I> {
    fn name(&self) -> &'static str {
        "appview"
    }

    async fn project(&self, author: &str, revision: &str, events: &[SyncEvent]) -> Result<()> {
        // One URI may change more than once in a batch; only its final state
        // is worth sending.
        let mut final_records: BTreeMap<&str, &SyncEvent> = BTreeMap::new();
        for event in events {
            final_records.insert(event.uri(), event);
        }
        let ops: Vec<ProjectRecord> = final_records
            .into_values()
            .filter_map(|event| match event {
                SyncEvent::PostCreated {
                    uri, cid, record, ..
                } => Some(self.op(
                    author,
                    uri,
                    Some(cid),
                    revision,
                    ProjectionOperation::Create,
                    POST_COLLECTION,
                    Some(record.clone()),
                )),
                SyncEvent::PostDeleted { uri, .. } => Some(self.op(
                    author,
                    uri,
                    None,
                    revision,
                    ProjectionOperation::Delete,
                    POST_COLLECTION,
                    None,
                )),
                SyncEvent::LikeCreated {
                    uri, cid, record, ..
                } => Some(self.op(
                    author,
                    uri,
                    Some(cid),
                    revision,
                    ProjectionOperation::Create,
                    LIKE_COLLECTION,
                    Some(record.clone()),
                )),
                SyncEvent::LikeDeleted { uri, .. } => Some(self.op(
                    author,
                    uri,
                    None,
                    revision,
                    ProjectionOperation::Delete,
                    LIKE_COLLECTION,
                    None,
                )),
                SyncEvent::ModerationAction {
                    uri,
                    record: Some(record),
                    ..
                } => self.moderation_op(author, revision, uri, record),
                SyncEvent::ModerationAction { record: None, .. } => None,
            })
            .collect();
        if ops.is_empty() {
            return Ok(());
        }
        self.ingress
            .project_records(&ProjectRecordsRequest { ops })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feeds::tests::{post_created, CapturingIngress, AUTHOR, SPACE};
    use serde_json::json;

    fn like_created(rkey: &str) -> SyncEvent {
        SyncEvent::LikeCreated {
            uri: format!("{SPACE}/{AUTHOR}/{LIKE_COLLECTION}/{rkey}"),
            author: AUTHOR.to_string(),
            cid: "bafylike".to_string(),
            rev: "3krev".to_string(),
            record: json!({
                "$type": LIKE_COLLECTION,
                "subject": {"uri": format!("{SPACE}/{AUTHOR}/{POST_COLLECTION}/3ka"), "cid": "bafypost"},
                "createdAt": "2026-08-19T00:00:00Z",
            }),
        }
    }

    #[tokio::test]
    async fn posts_likes_and_removals_all_project() {
        let projector = AppviewProjector::new(CapturingIngress::default(), SPACE);
        let action_uri = format!("{SPACE}/did:plc:community/{MODERATION_ACTION_COLLECTION}/3m");
        projector
            .project(
                AUTHOR,
                "3krev",
                &[
                    post_created("3ka", "hello"),
                    like_created("3kl"),
                    SyncEvent::ModerationAction {
                        uri: action_uri.clone(),
                        rev: "3krev".to_string(),
                        record: Some(json!({
                            "val": "remove",
                            "subject": {"uri": format!("{SPACE}/{AUTHOR}/{POST_COLLECTION}/3ka"), "cid": "bafypost"},
                        })),
                    },
                ],
            )
            .await
            .unwrap();

        let sent = projector.ingress.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        let ops = &sent[0].ops;
        assert_eq!(ops.len(), 3);
        let collections: Vec<&str> = ops.iter().map(|op| op.collection.as_str()).collect();
        assert!(collections.contains(&POST_COLLECTION));
        assert!(collections.contains(&LIKE_COLLECTION));
        assert!(collections.contains(&MODERATION_ACTION_COLLECTION));
        let flagged = ops
            .iter()
            .find(|op| op.collection == MODERATION_ACTION_COLLECTION)
            .unwrap();
        assert_eq!(flagged.operation, ProjectionOperation::Flag);
        assert_eq!(flagged.action_uri.as_deref(), Some(action_uri.as_str()));
        let like = ops
            .iter()
            .find(|op| op.collection == LIKE_COLLECTION)
            .unwrap();
        assert!(like.record.as_ref().unwrap()["subject"]["uri"]
            .as_str()
            .unwrap()
            .contains("/space/"));
    }

    #[tokio::test]
    async fn only_the_final_state_of_a_uri_is_sent() {
        let projector = AppviewProjector::new(CapturingIngress::default(), SPACE);
        let uri = format!("{SPACE}/{AUTHOR}/{POST_COLLECTION}/3ka");
        projector
            .project(
                AUTHOR,
                "3krev",
                &[
                    post_created("3ka", "first"),
                    SyncEvent::PostDeleted {
                        uri: uri.clone(),
                        author: AUTHOR.to_string(),
                    },
                ],
            )
            .await
            .unwrap();

        let sent = projector.ingress.sent.lock().unwrap();
        assert_eq!(sent[0].ops.len(), 1);
        assert_eq!(sent[0].ops[0].operation, ProjectionOperation::Delete);
    }

    #[tokio::test]
    async fn a_batch_with_nothing_to_send_makes_no_request() {
        let projector = AppviewProjector::new(CapturingIngress::default(), SPACE);
        projector
            .project(
                AUTHOR,
                "3krev",
                &[SyncEvent::ModerationAction {
                    uri: format!("{SPACE}/did:plc:community/{MODERATION_ACTION_COLLECTION}/3m"),
                    rev: "3krev".to_string(),
                    record: None,
                }],
            )
            .await
            .unwrap();
        assert!(projector.ingress.sent.lock().unwrap().is_empty());
    }
}
