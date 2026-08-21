//! Typed routing over an authenticated batch.
//!
//! A projection cares about *what happened*, not about record paths, so the
//! router turns index mutations into events before anything downstream sees
//! them. Routing runs on verified batches only — an unroutable or unknown
//! record is dropped and logged rather than raised, since one record a build
//! has never heard of must not stall a whole repo.

use rsky_space::record::decode_record;
use rsky_space::space_id::{RecordId, SpaceId};
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::debug;

use crate::index::IndexMutation;

/// Records on collections this build projects that were lost anyway because
/// their bytes would not decode. Dropping unknown collections is policy;
/// losing a known collection is data loss and must alarm.
pub static KNOWN_COLLECTION_DECODE_FAILURES: AtomicU64 = AtomicU64::new(0);

/// Stable event name for the loss alarm, for log-based alerting.
pub const LOSS_EVENT: &str = "space_known_collection_decode_failure";

pub const POST_COLLECTION: &str = "app.bsky.feed.post";
pub const LIKE_COLLECTION: &str = "app.bsky.feed.like";
pub const MODERATION_ACTION_COLLECTION: &str = "community.blacksky.moderation.action";

/// One routed change, in the terms a projection acts on.
#[derive(Debug, Clone, PartialEq)]
pub enum SyncEvent {
    PostCreated {
        uri: String,
        author: String,
        cid: String,
        rev: String,
        record: Value,
    },
    PostDeleted {
        uri: String,
        author: String,
    },
    LikeCreated {
        uri: String,
        author: String,
        cid: String,
        rev: String,
        record: Value,
    },
    LikeDeleted {
        uri: String,
        author: String,
    },
    /// A moderation action from the space authority's own repo. The router
    /// emits this only for the authority (D25), so a projection never has to
    /// re-check who wrote it. A lift is its own signed record (`neg: true`),
    /// not deletion of the action it reverses.
    ModerationAction {
        uri: String,
        rev: String,
        record: Option<Value>,
    },
}

impl SyncEvent {
    pub fn uri(&self) -> &str {
        match self {
            Self::PostCreated { uri, .. }
            | Self::PostDeleted { uri, .. }
            | Self::LikeCreated { uri, .. }
            | Self::LikeDeleted { uri, .. }
            | Self::ModerationAction { uri, .. } => uri,
        }
    }
}

/// Turns the mutations of one author's verified batch into events.
pub struct Router {
    space: SpaceId,
    authority: String,
}

impl Router {
    pub fn new(space: SpaceId, authority: impl Into<String>) -> Self {
        Self {
            space,
            authority: authority.into(),
        }
    }

    pub fn space(&self) -> &SpaceId {
        &self.space
    }

    pub fn route_batch(&self, did: &str, mutations: &[IndexMutation]) -> Vec<SyncEvent> {
        mutations
            .iter()
            .filter_map(|m| self.route(did, m))
            .collect()
    }

    pub fn route(&self, did: &str, mutation: &IndexMutation) -> Option<SyncEvent> {
        let uri = RecordId {
            space: self.space.clone(),
            author: did.to_string(),
            collection: mutation.collection().to_string(),
            rkey: mutation.rkey().to_string(),
        }
        .uri();

        match (mutation.collection(), mutation) {
            (POST_COLLECTION, IndexMutation::Delete { .. }) => Some(SyncEvent::PostDeleted {
                uri,
                author: did.to_string(),
            }),
            (LIKE_COLLECTION, IndexMutation::Delete { .. }) => Some(SyncEvent::LikeDeleted {
                uri,
                author: did.to_string(),
            }),
            (
                POST_COLLECTION | LIKE_COLLECTION,
                IndexMutation::Upsert {
                    cid, rev, value, ..
                },
            ) => {
                let record = decode_value(&uri, value.as_deref())?;
                let is_post = mutation.collection() == POST_COLLECTION;
                Some(if is_post {
                    SyncEvent::PostCreated {
                        uri,
                        author: did.to_string(),
                        cid: cid.clone(),
                        rev: rev.clone(),
                        record,
                    }
                } else {
                    SyncEvent::LikeCreated {
                        uri,
                        author: did.to_string(),
                        cid: cid.clone(),
                        rev: rev.clone(),
                        record,
                    }
                })
            }
            (MODERATION_ACTION_COLLECTION, _) if did != self.authority => {
                debug!(%uri, %did, "dropping moderation action from a repo that is not the authority's");
                None
            }
            (MODERATION_ACTION_COLLECTION, IndexMutation::Delete { .. }) => {
                Some(SyncEvent::ModerationAction {
                    uri,
                    rev: String::new(),
                    record: None,
                })
            }
            (MODERATION_ACTION_COLLECTION, IndexMutation::Upsert { rev, value, .. }) => {
                let record = decode_value(&uri, value.as_deref())?;
                Some(SyncEvent::ModerationAction {
                    uri,
                    rev: rev.clone(),
                    record: Some(record),
                })
            }
            (other, _) => {
                debug!(%uri, collection = %other, "dropping a collection this build does not project");
                None
            }
        }
    }
}

/// The two sync paths inline a record's value in different encodings: the
/// oplog carries it as JSON, a full-state CAR as DAG-CBOR. Either may be what
/// the index holds for a given record, so both are accepted here.
fn decode_inlined(bytes: &[u8]) -> std::result::Result<Value, String> {
    match decode_record(bytes) {
        Ok(value) if value.is_object() => Ok(value),
        cbor => serde_json::from_slice::<Value>(bytes)
            .map_err(|json_err| match cbor {
                Ok(_) => format!("dag-cbor value is not a record; json: {json_err}"),
                Err(cbor_err) => format!("dag-cbor: {cbor_err}; json: {json_err}"),
            })
            .and_then(|value| {
                if value.is_object() {
                    Ok(value)
                } else {
                    Err("value is not a record".to_string())
                }
            }),
    }
}

fn decode_value(uri: &str, value: Option<&[u8]>) -> Option<Value> {
    match value {
        Some(bytes) => match decode_inlined(bytes) {
            Ok(value) => Some(value),
            Err(err) => {
                let total = KNOWN_COLLECTION_DECODE_FAILURES.fetch_add(1, Ordering::Relaxed) + 1;
                tracing::error!(
                    event = LOSS_EVENT,
                    %uri,
                    %err,
                    total,
                    "a record on a projected collection will not decode and is lost"
                );
                None
            }
        },
        None => {
            // The host inlines a record's value with its op; without one there
            // is nothing to project, and the sweep will bring it back.
            debug!(%uri, "dropping a write carrying no record value");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsky_space::record::encode_record;
    use serde_json::json;

    const AUTHORITY: &str = "did:plc:community";
    const MEMBER: &str = "did:plc:alice";

    fn router() -> Router {
        Router::new(
            SpaceId::new(AUTHORITY, "community.blacksky.feed", "private"),
            AUTHORITY,
        )
    }

    fn upsert(collection: &str, value: Value) -> IndexMutation {
        IndexMutation::Upsert {
            collection: collection.to_string(),
            rkey: "3krkey".to_string(),
            cid: "bafyrecord".to_string(),
            rev: "3krev".to_string(),
            value: Some(encode_record(&value, 64 * 1024).unwrap()),
        }
    }

    fn delete(collection: &str) -> IndexMutation {
        IndexMutation::Delete {
            collection: collection.to_string(),
            rkey: "3krkey".to_string(),
        }
    }

    fn post() -> Value {
        json!({"$type": POST_COLLECTION, "text": "hello", "createdAt": "2026-08-09T00:00:00.000Z"})
    }

    #[test]
    fn a_post_routes_to_its_space_uri() {
        match router().route(MEMBER, &upsert(POST_COLLECTION, post())) {
            Some(SyncEvent::PostCreated { uri, record, .. }) => {
                assert_eq!(
                    uri,
                    format!("at://{AUTHORITY}/space/community.blacksky.feed/private/{MEMBER}/{POST_COLLECTION}/3krkey")
                );
                assert_eq!(record["text"], "hello");
            }
            other => panic!("expected a post create, got {other:?}"),
        }
    }

    #[test]
    fn a_like_over_a_space_subject_survives_routing() {
        // The record standard lexicon validation would reject: its subject is
        // a space URI, which is not an at-uri (D29).
        let like = json!({
            "$type": LIKE_COLLECTION,
            "subject": {"uri": format!("at://{AUTHORITY}/space/x/y/{MEMBER}/{POST_COLLECTION}/3kz"), "cid": "bafy"},
            "createdAt": "2026-08-09T00:00:00.000Z",
        });
        match router().route(MEMBER, &upsert(LIKE_COLLECTION, like)) {
            Some(SyncEvent::LikeCreated { record, .. }) => {
                assert!(record["subject"]["uri"]
                    .as_str()
                    .unwrap()
                    .contains("/space/"));
            }
            other => panic!("expected a like create, got {other:?}"),
        }
    }

    #[test]
    fn deletes_route_by_kind() {
        let r = router();
        assert!(matches!(
            r.route(MEMBER, &delete(POST_COLLECTION)),
            Some(SyncEvent::PostDeleted { .. })
        ));
        assert!(matches!(
            r.route(MEMBER, &delete(LIKE_COLLECTION)),
            Some(SyncEvent::LikeDeleted { .. })
        ));
    }

    #[test]
    fn moderation_actions_come_only_from_the_authority() {
        let action = json!({
            "$type": MODERATION_ACTION_COLLECTION,
            "subject": {"uri": "at://a/space/t/s/did:plc:alice/app.bsky.feed.post/3k"},
            "event": {"$type": "community.blacksky.moderation.action#delete"},
            "createdAt": "2026-08-09T00:00:00.000Z",
        });
        let r = router();
        assert!(matches!(
            r.route(
                AUTHORITY,
                &upsert(MODERATION_ACTION_COLLECTION, action.clone())
            ),
            Some(SyncEvent::ModerationAction {
                record: Some(_),
                ..
            })
        ));
        // A member's own copy of the same record is a forgery attempt.
        assert!(r
            .route(MEMBER, &upsert(MODERATION_ACTION_COLLECTION, action))
            .is_none());
        // Deletes do not lift an effect: only a signed negation may do that.
        assert!(matches!(
            r.route(AUTHORITY, &delete(MODERATION_ACTION_COLLECTION)),
            Some(SyncEvent::ModerationAction { record: None, .. })
        ));
    }

    #[test]
    fn a_value_inlined_as_json_routes_like_one_inlined_as_dag_cbor() {
        let mut json_valued = upsert(POST_COLLECTION, post());
        if let IndexMutation::Upsert { value, .. } = &mut json_valued {
            *value = Some(serde_json::to_vec(&post()).unwrap());
        }
        match router().route(MEMBER, &json_valued) {
            Some(SyncEvent::PostCreated { record, .. }) => assert_eq!(record["text"], "hello"),
            other => panic!("expected a post create, got {other:?}"),
        }
    }

    #[test]
    fn only_unknown_collections_may_drop_silently() {
        let r = router();

        // Unknown collection: drop-and-log is policy, no alarm.
        let before = KNOWN_COLLECTION_DECODE_FAILURES.load(Ordering::SeqCst);
        assert!(r
            .route(
                MEMBER,
                &upsert("app.bsky.graph.follow", json!({"$type": "x"}))
            )
            .is_none());
        assert_eq!(
            KNOWN_COLLECTION_DECODE_FAILURES.load(Ordering::SeqCst),
            before
        );

        // A known collection failing to decode is data loss: it must raise
        // the alarmed counter, never vanish silently.
        let mut broken = upsert(POST_COLLECTION, post());
        if let IndexMutation::Upsert { value, .. } = &mut broken {
            *value = Some(vec![0xff, 0xff]);
        }
        assert!(r.route(MEMBER, &broken).is_none());
        assert_eq!(
            KNOWN_COLLECTION_DECODE_FAILURES.load(Ordering::SeqCst),
            before + 1
        );
    }
}
