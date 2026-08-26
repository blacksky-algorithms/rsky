//! Journal-driven projection delivery.

use std::collections::HashSet;
use std::sync::Arc;

use crate::error::Result;
use crate::index::SpaceIndex;
use crate::projection::Projector;
use crate::router::Router;

pub const DEAD_LETTER_AFTER: u32 = 3;
pub const DENIAL_PARK_AFTER: u32 = 20;

/// Which batches a drain pass will attempt. Denied batches wait for a sweep:
/// admission changes at the pace of a membership write, not of a drain tick.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lane {
    Fast,
    Sweep,
}

/// Reads the batch journal for one projector and advances only after its
/// destination has accepted the batch. Each projector gets an independent
/// cursor, so a failed destination cannot hold the index or another
/// destination hostage.
pub struct JournalConsumer {
    name: &'static str,
    router: Router,
    projector: Box<dyn Projector>,
    dead_letter_after: u32,
    denial_park_after: u32,
}

impl JournalConsumer {
    pub fn new(router: Router, projector: Box<dyn Projector>) -> Self {
        Self {
            name: projector.name(),
            router,
            projector,
            dead_letter_after: DEAD_LETTER_AFTER,
            denial_park_after: DENIAL_PARK_AFTER,
        }
    }

    pub fn with_dead_letter_after(mut self, attempts: u32) -> Self {
        self.dead_letter_after = attempts;
        self
    }

    pub fn with_denial_park_after(mut self, sweeps: u32) -> Self {
        self.denial_park_after = sweeps;
        self
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    async fn drain_with_status(&self, index: &dyn SpaceIndex, lane: Lane) -> Result<(usize, bool)> {
        let mut delivered = 0;
        let mut succeeded = true;
        // An author whose batch was denied keeps its remaining batches in
        // order: delivering a later one would advance the cursor past the
        // denied batch and lose it for good once admission arrives.
        let mut denied_authors: HashSet<String> = HashSet::new();
        for batch in index.pending_batches(self.name).await? {
            if denied_authors.contains(&batch.author) {
                continue;
            }
            if batch.denials > 0 && lane == Lane::Fast {
                succeeded = false;
                denied_authors.insert(batch.author.clone());
                continue;
            }
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
                    if error.is_admission_denied() {
                        let denials = index
                            .record_projection_denial(
                                self.name,
                                &batch.author,
                                &batch.rev,
                                &error.to_string(),
                                self.denial_park_after,
                            )
                            .await?;
                        denied_authors.insert(batch.author.clone());
                        if denials >= self.denial_park_after {
                            tracing::error!(projector = self.name, space = %self.router.space().uri(), author = %batch.author, rev = %batch.rev, denials, error = %error, "projection batch parked: admission never granted");
                        } else {
                            tracing::warn!(projector = self.name, space = %self.router.space().uri(), author = %batch.author, rev = %batch.rev, denials, error = %error, "author not admitted; batch retries on a later sweep");
                        }
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
        self.drain_with_status(index, Lane::Fast)
            .await
            .map(|(delivered, _)| delivered)
    }

    /// A drain that also re-attempts denied batches. Called once per sweep.
    pub async fn drain_sweep(&self, index: &dyn SpaceIndex) -> Result<usize> {
        self.drain_with_status(index, Lane::Sweep)
            .await
            .map(|(delivered, _)| delivered)
    }

    pub async fn drain_succeeded(&self, index: &dyn SpaceIndex) -> Result<bool> {
        self.drain_with_status(index, Lane::Fast)
            .await
            .map(|(_, succeeded)| succeeded)
    }

    pub async fn drain_sweep_succeeded(&self, index: &dyn SpaceIndex) -> Result<bool> {
        self.drain_with_status(index, Lane::Sweep)
            .await
            .map(|(_, succeeded)| succeeded)
    }
}

pub type SharedJournalConsumer = Arc<JournalConsumer>;

/// Drain every projector, then drop the journal rows all of them have passed.
/// Returns whether every destination accepted everything pending for it.
pub async fn drain_all(index: &dyn SpaceIndex, consumers: &[SharedJournalConsumer]) -> bool {
    drain_all_in(index, consumers, Lane::Fast).await
}

/// [`drain_all`] plus a re-attempt of every denied batch.
pub async fn drain_all_sweep(index: &dyn SpaceIndex, consumers: &[SharedJournalConsumer]) -> bool {
    drain_all_in(index, consumers, Lane::Sweep).await
}

async fn drain_all_in(
    index: &dyn SpaceIndex,
    consumers: &[SharedJournalConsumer],
    lane: Lane,
) -> bool {
    let mut succeeded = true;
    for consumer in consumers {
        let drained = match lane {
            Lane::Fast => consumer.drain_succeeded(index).await,
            Lane::Sweep => consumer.drain_sweep_succeeded(index).await,
        };
        match drained {
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
    const BOB: &str = "did:plc:bob";

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
        /// Authors this destination refuses admission; cleared to admit them.
        denied: Mutex<Vec<String>>,
        attempts: Mutex<Vec<(String, String)>>,
    }

    #[async_trait]
    impl Projector for Recorder {
        fn name(&self) -> &'static str {
            "recorder"
        }
        async fn project(&self, did: &str, rev: &str, events: &[SyncEvent]) -> Result<()> {
            self.attempts
                .lock()
                .unwrap()
                .push((did.to_string(), rev.to_string()));
            if self.denied.lock().unwrap().iter().any(|d| d == did) {
                return Err(DaemonError::AdmissionDenied(
                    "appview projectRecords returned 401 Unauthorized".to_string(),
                ));
            }
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

    /// A handle onto a [`Recorder`] the test keeps, so admission can flip
    /// between drains.
    struct Shared(Arc<Recorder>);

    #[async_trait]
    impl Projector for Shared {
        fn name(&self) -> &'static str {
            "recorder"
        }
        async fn project(&self, did: &str, rev: &str, events: &[SyncEvent]) -> Result<()> {
            self.0.project(did, rev, events).await
        }
    }

    fn shared_consumer(recorder: &Arc<Recorder>) -> JournalConsumer {
        JournalConsumer::new(router(), Box::new(Shared(recorder.clone())))
    }

    async fn journal(index: &InMemoryIndex, author: &str, rev: &str, rkey: &str) {
        index
            .journal_batch(author, rev, &[post_mutation(rkey)])
            .await
            .unwrap();
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

    #[tokio::test]
    async fn a_denied_batch_projects_once_when_admission_arrives() {
        let index = InMemoryIndex::new();
        journal(&index, BOB, "3krev", "3ka").await;
        let recorder = Arc::new(Recorder::default());
        recorder.denied.lock().unwrap().push(BOB.to_string());
        let consumer = shared_consumer(&recorder).with_denial_park_after(5);

        assert_eq!(consumer.drain_sweep(&index).await.unwrap(), 0);
        assert_eq!(index.pending_batches("recorder").await.unwrap().len(), 1);
        assert_eq!(recorder.attempts.lock().unwrap().len(), 1);

        // The fast lane leaves a denied batch alone: admission moves at the
        // pace of a membership write, not of a drain tick.
        assert_eq!(consumer.drain(&index).await.unwrap(), 0);
        assert_eq!(recorder.attempts.lock().unwrap().len(), 1);

        recorder.denied.lock().unwrap().clear();
        assert_eq!(consumer.drain_sweep(&index).await.unwrap(), 1);
        assert_eq!(consumer.drain_sweep(&index).await.unwrap(), 0);
        assert_eq!(recorder.delivered.lock().unwrap().len(), 1);
        assert!(index.pending_batches("recorder").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_denial_that_never_lifts_parks_after_its_budget() {
        let index = InMemoryIndex::new();
        journal(&index, BOB, "3krev", "3ka").await;
        let recorder = Arc::new(Recorder::default());
        recorder.denied.lock().unwrap().push(BOB.to_string());
        let consumer = shared_consumer(&recorder).with_denial_park_after(3);

        for _ in 0..3 {
            assert!(!consumer.drain_sweep_succeeded(&index).await.unwrap());
        }
        assert_eq!(recorder.attempts.lock().unwrap().len(), 3);
        assert!(index.pending_batches("recorder").await.unwrap().is_empty());

        for _ in 0..2 {
            assert_eq!(consumer.drain_sweep(&index).await.unwrap(), 0);
        }
        assert_eq!(
            recorder.attempts.lock().unwrap().len(),
            3,
            "a parked batch is never attempted again"
        );
    }

    #[tokio::test]
    async fn a_denial_does_not_spend_the_poison_budget() {
        let index = InMemoryIndex::new();
        journal(&index, BOB, "3krev", "3ka").await;
        let recorder = Arc::new(Recorder::default());
        recorder.denied.lock().unwrap().push(BOB.to_string());
        let consumer = shared_consumer(&recorder)
            .with_dead_letter_after(1)
            .with_denial_park_after(10);

        for _ in 0..4 {
            assert_eq!(consumer.drain_sweep(&index).await.unwrap(), 0);
        }
        assert_eq!(
            index.pending_batches("recorder").await.unwrap().len(),
            1,
            "the one-attempt poison budget must not dead-letter a denial"
        );

        // The same budget still kills a genuinely poisoned batch on its first
        // failure.
        let index = InMemoryIndex::new();
        journal(&index, AUTHOR, "3krev", "3ka").await;
        let poison = Recorder::default();
        poison.fail_next.store(5, Ordering::SeqCst);
        let consumer = JournalConsumer::new(router(), Box::new(poison)).with_dead_letter_after(1);
        assert_eq!(consumer.drain_sweep(&index).await.unwrap(), 0);
        assert!(index.pending_batches("recorder").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_denial_holds_its_author_in_order_and_leaves_others_alone() {
        let index = InMemoryIndex::new();
        journal(&index, AUTHOR, "3krev1", "3ka").await;
        journal(&index, AUTHOR, "3krev2", "3kb").await;
        journal(&index, BOB, "3krev3", "3kc").await;
        journal(&index, BOB, "3krev4", "3kd").await;
        let recorder = Arc::new(Recorder::default());
        recorder.denied.lock().unwrap().push(BOB.to_string());
        let consumer = shared_consumer(&recorder).with_denial_park_after(5);

        assert_eq!(consumer.drain_sweep(&index).await.unwrap(), 2);
        assert_eq!(
            *recorder.attempts.lock().unwrap(),
            vec![
                (AUTHOR.to_string(), "3krev1".to_string()),
                (AUTHOR.to_string(), "3krev2".to_string()),
                (BOB.to_string(), "3krev3".to_string()),
            ],
            "bob's later batch must not overtake his denied one"
        );

        recorder.denied.lock().unwrap().clear();
        assert_eq!(consumer.drain_sweep(&index).await.unwrap(), 2);
        let delivered: Vec<String> = recorder
            .delivered
            .lock()
            .unwrap()
            .iter()
            .map(|(rev, _)| rev.clone())
            .collect();
        assert_eq!(delivered, vec!["3krev1", "3krev2", "3krev3", "3krev4"]);
    }
}
