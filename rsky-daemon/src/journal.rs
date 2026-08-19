//! Journal-driven projection delivery.

use std::sync::Arc;

use crate::error::Result;
use crate::index::SpaceIndex;
use crate::projection::Projector;
use crate::router::Router;

pub const DEAD_LETTER_AFTER: u32 = 3;

/// Reads the batch journal for one projector and advances only after its
/// destination has accepted the batch. Each projector gets an independent
/// cursor, so a failed destination cannot hold the index or another
/// destination hostage.
pub struct JournalConsumer {
    name: &'static str,
    router: Router,
    projector: Box<dyn Projector>,
    dead_letter_after: u32,
}

impl JournalConsumer {
    pub fn new(router: Router, projector: Box<dyn Projector>) -> Self {
        Self {
            name: projector.name(),
            router,
            projector,
            dead_letter_after: DEAD_LETTER_AFTER,
        }
    }

    pub fn with_dead_letter_after(mut self, attempts: u32) -> Self {
        self.dead_letter_after = attempts;
        self
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    async fn drain_with_status(&self, index: &dyn SpaceIndex) -> Result<(usize, bool)> {
        let mut delivered = 0;
        let mut succeeded = true;
        for batch in index.pending_batches(self.name).await? {
            let events = self.router.route_batch(&batch.author, &batch.mutations);
            let result = if events.is_empty() {
                Ok(())
            } else {
                self.projector
                    .project(&batch.author, &batch.rev, &events)
                    .await
            };
            match result {
                Ok(()) => {
                    index
                        .advance_projector_cursor(self.name, &batch.author, &batch.rev)
                        .await?;
                    delivered += 1;
                }
                Err(error) => {
                    succeeded = false;
                    if error.is_retryable_projection() {
                        tracing::warn!(projector = self.name, space = %self.router.space().uri(), author = %batch.author, rev = %batch.rev, error = %error, "projection destination unavailable; batch remains pending without consuming its failure budget");
                        continue;
                    }
                    let attempts = index
                        .record_projection_failure(
                            self.name,
                            &batch.author,
                            &batch.rev,
                            &error.to_string(),
                            self.dead_letter_after,
                        )
                        .await?;
                    if attempts >= self.dead_letter_after {
                        tracing::error!(projector = self.name, space = %self.router.space().uri(), author = %batch.author, rev = %batch.rev, attempts, error = %error, "projection batch dead-lettered");
                    } else {
                        tracing::warn!(projector = self.name, space = %self.router.space().uri(), author = %batch.author, rev = %batch.rev, attempts, error = %error, "projection batch will retry");
                    }
                }
            }
        }
        Ok((delivered, succeeded))
    }

    pub async fn drain(&self, index: &dyn SpaceIndex) -> Result<usize> {
        self.drain_with_status(index)
            .await
            .map(|(delivered, _)| delivered)
    }

    pub async fn drain_succeeded(&self, index: &dyn SpaceIndex) -> Result<bool> {
        self.drain_with_status(index)
            .await
            .map(|(_, succeeded)| succeeded)
    }
}

pub type SharedJournalConsumer = Arc<JournalConsumer>;

/// Drain every projector, then drop the journal rows all of them have passed.
/// Returns whether every destination accepted everything pending for it.
pub async fn drain_all(index: &dyn SpaceIndex, consumers: &[SharedJournalConsumer]) -> bool {
    let mut succeeded = true;
    for consumer in consumers {
        match consumer.drain_succeeded(index).await {
            Ok(clean) => succeeded &= clean,
            Err(error) => {
                succeeded = false;
                tracing::warn!(projector = consumer.name(), error = %error, "projection drain failed");
            }
        }
    }
    if consumers.is_empty() {
        return succeeded;
    }
    let names: Vec<&str> = consumers.iter().map(|c| c.name()).collect();
    if let Err(error) = index.prune_journal(&names).await {
        tracing::warn!(error = %error, "journal prune failed");
    }
    succeeded
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::DaemonError;
    use crate::index::{InMemoryIndex, IndexMutation};
    use crate::router::{SyncEvent, POST_COLLECTION};
    use async_trait::async_trait;
    use rsky_space::record::encode_record;
    use rsky_space::space_id::SpaceId;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    const AUTHORITY: &str = "did:plc:community";
    const AUTHOR: &str = "did:plc:alice";

    fn router() -> Router {
        Router::new(
            SpaceId::new(AUTHORITY, "community.blacksky.feed", "private"),
            AUTHORITY,
        )
    }

    fn post_mutation(rkey: &str) -> IndexMutation {
        IndexMutation::Upsert {
            collection: POST_COLLECTION.to_string(),
            rkey: rkey.to_string(),
            cid: "bafypost".to_string(),
            rev: "3krev".to_string(),
            value: Some(
                encode_record(
                    &json!({"$type": POST_COLLECTION, "text": "hi", "createdAt": "2026-08-19T00:00:00Z"}),
                    64 * 1024,
                )
                .unwrap(),
            ),
        }
    }

    #[derive(Default)]
    struct Recorder {
        delivered: Mutex<Vec<(String, usize)>>,
        fail_next: AtomicUsize,
        retryable: bool,
    }

    #[async_trait]
    impl Projector for Recorder {
        fn name(&self) -> &'static str {
            "recorder"
        }
        async fn project(&self, _did: &str, rev: &str, events: &[SyncEvent]) -> Result<()> {
            if self.fail_next.load(Ordering::SeqCst) > 0 {
                self.fail_next.fetch_sub(1, Ordering::SeqCst);
                return Err(if self.retryable {
                    DaemonError::RetryableProjection("destination down".to_string())
                } else {
                    DaemonError::Xrpc("rejected".to_string())
                });
            }
            self.delivered
                .lock()
                .unwrap()
                .push((rev.to_string(), events.len()));
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_delivered_batch_advances_its_cursor_once() {
        let index = InMemoryIndex::new();
        index
            .journal_batch(AUTHOR, "3krev", &[post_mutation("3ka")])
            .await
            .unwrap();
        let consumer = JournalConsumer::new(router(), Box::<Recorder>::default());

        assert_eq!(consumer.drain(&index).await.unwrap(), 1);
        assert_eq!(consumer.drain(&index).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn a_retryable_failure_keeps_the_batch_without_spending_its_budget() {
        let index = InMemoryIndex::new();
        index
            .journal_batch(AUTHOR, "3krev", &[post_mutation("3ka")])
            .await
            .unwrap();
        let projector = Recorder {
            retryable: true,
            ..Default::default()
        };
        projector.fail_next.store(1, Ordering::SeqCst);
        let consumer =
            JournalConsumer::new(router(), Box::new(projector)).with_dead_letter_after(1);

        assert!(!consumer.drain_succeeded(&index).await.unwrap());
        assert_eq!(index.pending_batches("recorder").await.unwrap().len(), 1);
        assert_eq!(consumer.drain(&index).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn a_rejected_batch_dead_letters_after_its_budget() {
        let index = InMemoryIndex::new();
        index
            .journal_batch(AUTHOR, "3krev", &[post_mutation("3ka")])
            .await
            .unwrap();
        let projector = Recorder::default();
        projector.fail_next.store(5, Ordering::SeqCst);
        let consumer =
            JournalConsumer::new(router(), Box::new(projector)).with_dead_letter_after(2);

        assert_eq!(consumer.drain(&index).await.unwrap(), 0);
        assert_eq!(index.pending_batches("recorder").await.unwrap().len(), 1);
        assert_eq!(consumer.drain(&index).await.unwrap(), 0);
        assert!(index.pending_batches("recorder").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_batch_with_nothing_to_project_still_advances() {
        let index = InMemoryIndex::new();
        index
            .journal_batch(
                AUTHOR,
                "3krev",
                &[IndexMutation::Delete {
                    collection: "app.bsky.graph.follow".to_string(),
                    rkey: "3ka".to_string(),
                }],
            )
            .await
            .unwrap();
        let consumer = JournalConsumer::new(router(), Box::<Recorder>::default());

        assert_eq!(consumer.drain(&index).await.unwrap(), 1);
        assert!(index.pending_batches("recorder").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn one_stalled_projector_does_not_hold_back_another() {
        let index = InMemoryIndex::new();
        index
            .journal_batch(AUTHOR, "3krev", &[post_mutation("3ka")])
            .await
            .unwrap();

        struct Stalled;
        #[async_trait]
        impl Projector for Stalled {
            fn name(&self) -> &'static str {
                "stalled"
            }
            async fn project(&self, _did: &str, _rev: &str, _events: &[SyncEvent]) -> Result<()> {
                Err(DaemonError::RetryableProjection("down".to_string()))
            }
        }

        let healthy: SharedJournalConsumer =
            Arc::new(JournalConsumer::new(router(), Box::<Recorder>::default()));
        let stalled: SharedJournalConsumer =
            Arc::new(JournalConsumer::new(router(), Box::new(Stalled)));
        drain_all(&index, &[healthy.clone(), stalled.clone()]).await;

        assert!(index.pending_batches("recorder").await.unwrap().is_empty());
        assert_eq!(index.pending_batches("stalled").await.unwrap().len(), 1);
    }
}
