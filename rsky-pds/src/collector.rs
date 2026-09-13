//! The post-decommission blob collector.
//!
//! While another implementation can still read the object store nothing is
//! ever deleted; dereferenced objects and deleted accounts leave journal
//! rows instead. Once that is no longer true, this collector drains those
//! rows with a protocol that makes every terminal state provable: each
//! physical delete is one journaled attempt bound to the exact key it
//! names, the attempt is persisted before the request is sent, a request
//! the store never confirmed stays ambiguous, and an ambiguous delete
//! retires its key to the generation registry so the content can never be
//! stored under that key again. A deleted account's namespaces are listed
//! by prefix and declared purged only when every object is gone and every
//! journaled write has a confirmed outcome; a namespace another
//! implementation may have written is never more than observed empty, and
//! is listed again on a schedule.

use crate::actor_store::blob::{BlobWork, BlobWorkKind, BlobWorkState};
use crate::actor_store::blobstore::{BlobStore, BlobstoreFactory, DeleteError, ObjectKind};
use crate::actor_store::ActorStore;
use crate::blob_attempts::{AttemptJournal, AttemptOutcome};
use crate::blob_generations::Generations;
use crate::lifecycle::{LifecycleStore, PurgeObligation};
use anyhow::{bail, Result};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use rsky_common::time::from_str_to_utc;
use serde::Serialize;
use std::sync::Arc;

/// How long after a deletion request the weekly listing schedule lasts.
const LEGACY_WEEKLY_DAYS: i64 = 90;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum PurgeOutcome {
    /// Every prefix is empty and every write this process ever made to
    /// the namespace has a confirmed outcome; nothing can reappear.
    VerifiedPurged,
    /// Every prefix is empty but another implementation wrote here, or
    /// the namespace has no attempt history: a delayed legacy write cannot
    /// be excluded, so the namespace is listed again at `next_relist_at`.
    ObservedEmptyLegacyUncertain { next_relist_at: String },
    /// Something still stands in the way.
    Open { reason: String },
    /// An observed-empty namespace whose next listing is not due.
    Skipped { until: String },
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ActorReport {
    pub did: String,
    pub checked: usize,
    pub deleted: usize,
    pub superseded: usize,
    pub failed: usize,
    pub ambiguous: usize,
    pub retired: usize,
    pub purge: Option<PurgeOutcome>,
}

pub struct Collector {
    pub lifecycle: LifecycleStore,
    pub attempts: AttemptJournal,
    pub generations: Generations,
    /// Another implementation can still read the store: the collector
    /// refuses to do anything.
    pub coexistence: bool,
}

fn count(outcome: &str) {
    crate::metrics::METRICS
        .blob_collector_outcomes
        .with_label_values(&[outcome])
        .inc();
}

impl Collector {
    pub fn refuse_in_coexistence(&self) -> Result<()> {
        if self.coexistence {
            bail!("the blob collector never runs while another implementation shares the store (PDS_COEXISTENCE)");
        }
        Ok(())
    }

    /// A registry that does not know a key a journal row says was retired
    /// is an older copy; running against it could reuse a retired key.
    async fn verify_registry(
        &self,
        did: &str,
        rows: &[BlobWork],
        blobstore: &dyn BlobStore,
    ) -> Result<()> {
        for row in rows
            .iter()
            .filter(|row| row.state == BlobWorkState::Retired)
        {
            let key = blobstore.object_key(ObjectKind::Permanent, &row.key, 0);
            let generation = self
                .generations
                .current(did, row.cid.as_deref().unwrap_or(&row.key))
                .await?;
            if generation == 0 && !self.generations.is_retired(did, &key).await? {
                bail!(
                    "the blob generation registry is older than the journal: {key} was retired but the registry does not know it"
                );
            }
        }
        Ok(())
    }

    /// Runs one journaled delete against a physical key.
    async fn delete(
        &self,
        did: &str,
        blobstore: &dyn BlobStore,
        key: &str,
    ) -> Result<AttemptOutcome> {
        let id = self.attempts.begin(did, key, "delete").await?;
        let outcome = match blobstore.delete_object(key.to_owned()).await {
            Ok(()) => AttemptOutcome::Succeeded,
            Err(DeleteError::Definitive(reason)) => AttemptOutcome::Failed(reason),
            Err(DeleteError::Ambiguous(reason)) => AttemptOutcome::Ambiguous(reason),
        };
        self.attempts.resolve(id, outcome.clone()).await?;
        Ok(outcome)
    }

    fn key_for(&self, blobstore: &dyn BlobStore, row: &BlobWork, generation: u32) -> String {
        match row.kind {
            BlobWorkKind::Permanent => {
                blobstore.object_key(ObjectKind::Permanent, &row.key, generation)
            }
            BlobWorkKind::Temp => blobstore.object_key(ObjectKind::Temp, &row.key, 0),
            BlobWorkKind::Quarantine => blobstore.object_key(ObjectKind::Quarantine, &row.key, 0),
        }
    }

    /// Drains the actor's deferred deletions and, when the account was
    /// deleted, works on its purge obligation.
    pub async fn collect_actor(
        &self,
        actor_store: &ActorStore,
        blobstore: Arc<dyn BlobStore>,
        did: &str,
    ) -> Result<ActorReport> {
        self.refuse_in_coexistence()?;
        let mut report = ActorReport {
            did: did.to_owned(),
            ..ActorReport::default()
        };
        if let Ok(reader) = actor_store.read(did.to_owned(), blobstore.clone()).await {
            let rows = reader.blob.blob_work().await?;
            self.verify_registry(did, &rows, blobstore.as_ref()).await?;
            for row in rows {
                match row.state {
                    BlobWorkState::GcDeferred | BlobWorkState::Checking | BlobWorkState::Failed => {
                        report.checked += 1;
                        reader
                            .blob
                            .set_blob_work_state(row.id, BlobWorkState::Checking)
                            .await?;
                        if row.kind == BlobWorkKind::Permanent
                            && reader.blob.is_referenced(&row.key).await?
                        {
                            reader
                                .blob
                                .set_blob_work_state(row.id, BlobWorkState::Superseded)
                                .await?;
                            report.superseded += 1;
                            count("superseded");
                            continue;
                        }
                        let generation = match row.kind {
                            BlobWorkKind::Permanent => {
                                self.generations.current(did, &row.key).await?
                            }
                            _ => 0,
                        };
                        let key = self.key_for(blobstore.as_ref(), &row, generation);
                        reader
                            .blob
                            .set_blob_work_state(row.id, BlobWorkState::Issuing)
                            .await?;
                        match self.delete(did, blobstore.as_ref(), &key).await? {
                            AttemptOutcome::Succeeded => {
                                reader
                                    .blob
                                    .set_blob_work_state(row.id, BlobWorkState::Done)
                                    .await?;
                                report.deleted += 1;
                                count("deleted");
                            }
                            AttemptOutcome::Failed(reason) => {
                                tracing::warn!(did, key, %reason, "the store refused a delete; it will be tried again");
                                reader
                                    .blob
                                    .set_blob_work_state(row.id, BlobWorkState::Failed)
                                    .await?;
                                report.failed += 1;
                                count("failed");
                            }
                            AttemptOutcome::Ambiguous(reason) => {
                                tracing::warn!(did, key, %reason, "a delete was never confirmed; the key is retired");
                                reader
                                    .blob
                                    .set_blob_work_state(row.id, BlobWorkState::Ambiguous)
                                    .await?;
                                report.ambiguous += 1;
                                count("ambiguous");
                                self.retire(&reader.blob, did, &row, &key, &mut report)
                                    .await?;
                            }
                        }
                    }
                    BlobWorkState::Issuing => {
                        // the process died between journaling and recording
                        report.checked += 1;
                        let generation = match row.kind {
                            BlobWorkKind::Permanent => {
                                self.generations.current(did, &row.key).await?
                            }
                            _ => 0,
                        };
                        let key = self.key_for(blobstore.as_ref(), &row, generation);
                        let latest = self.attempts.latest(did, &key).await?;
                        match latest
                            .as_ref()
                            .and_then(|attempt| attempt.outcome.as_deref())
                        {
                            Some(outcome) if outcome.starts_with("succeeded") => {
                                reader
                                    .blob
                                    .set_blob_work_state(row.id, BlobWorkState::Done)
                                    .await?;
                                report.deleted += 1;
                            }
                            Some(outcome) if outcome.starts_with("failed") => {
                                reader
                                    .blob
                                    .set_blob_work_state(row.id, BlobWorkState::Failed)
                                    .await?;
                                report.failed += 1;
                            }
                            _ => {
                                reader
                                    .blob
                                    .set_blob_work_state(row.id, BlobWorkState::Ambiguous)
                                    .await?;
                                report.ambiguous += 1;
                                count("ambiguous");
                                self.retire(&reader.blob, did, &row, &key, &mut report)
                                    .await?;
                            }
                        }
                    }
                    BlobWorkState::Ambiguous => {
                        report.checked += 1;
                        let key = self.key_for(blobstore.as_ref(), &row, 0);
                        self.retire(&reader.blob, did, &row, &key, &mut report)
                            .await?;
                    }
                    _ => {}
                }
            }
        }
        if let Some(obligation) = self.lifecycle.purge_obligation_of(did).await? {
            let progress = self
                .lifecycle
                .purge_progress_of(did)
                .await?
                .unwrap_or_default();
            if progress.physically_purged_at.is_none() {
                report.purge = Some(
                    self.purge(&obligation, &progress.next_relist_at, blobstore.as_ref())
                        .await?,
                );
            }
        }
        Ok(report)
    }

    /// Retires the key of a permanent object whose delete was never
    /// confirmed; other kinds keep their ambiguous row for an operator.
    async fn retire(
        &self,
        blob: &crate::actor_store::blob::BlobReader,
        did: &str,
        row: &BlobWork,
        key: &str,
        report: &mut ActorReport,
    ) -> Result<()> {
        if row.kind != BlobWorkKind::Permanent {
            return Ok(());
        }
        if !self.generations.is_retired(did, key).await? {
            self.generations.retire(did, &row.key, key).await?;
        }
        blob.set_blob_work_state(row.id, BlobWorkState::Retired)
            .await?;
        report.retired += 1;
        count("retired");
        Ok(())
    }

    async fn purge(
        &self,
        obligation: &PurgeObligation,
        next_relist_at: &Option<String>,
        blobstore: &dyn BlobStore,
    ) -> Result<PurgeOutcome> {
        let now = Utc::now();
        if let Some(until) = next_relist_at {
            if from_str_to_utc(until).map(|due| due > now).unwrap_or(false) {
                return Ok(PurgeOutcome::Skipped {
                    until: until.clone(),
                });
            }
        }
        let did = &obligation.did;
        let prefixes = blobstore.namespace_prefixes();
        let mut refused = 0usize;
        for prefix in &prefixes {
            for key in blobstore.list_objects(prefix.clone()).await? {
                if let AttemptOutcome::Failed(reason) = self.delete(did, blobstore, &key).await? {
                    tracing::warn!(did, key, %reason, "the store refused a purge delete");
                    refused += 1;
                }
            }
        }
        if refused > 0 {
            count("purge-open");
            return Ok(PurgeOutcome::Open {
                reason: format!("{refused} deletes were refused"),
            });
        }
        let mut remaining = 0usize;
        for prefix in &prefixes {
            remaining += blobstore.list_objects(prefix.clone()).await?.len();
        }
        if remaining > 0 {
            count("purge-open");
            return Ok(PurgeOutcome::Open {
                reason: format!("{remaining} objects remain after deletion"),
            });
        }
        let unconfirmed_writes = self
            .attempts
            .unresolved(did)
            .await?
            .into_iter()
            .filter(|attempt| attempt.is_write())
            .count();
        if unconfirmed_writes > 0 {
            count("purge-open");
            return Ok(PurgeOutcome::Open {
                reason: format!(
                    "{unconfirmed_writes} writes to the namespace were never confirmed"
                ),
            });
        }
        let namespace = self.attempts.namespace_of(did).await?;
        let legacy_possible = namespace
            .as_ref()
            .map(|namespace| namespace.ts_era_writes_possible)
            .unwrap_or(true);
        if legacy_possible {
            let next = next_relist(&obligation.requested_at, now);
            self.lifecycle.mark_observed_empty(did, &next).await?;
            count("purge-observed-empty");
            Ok(PurgeOutcome::ObservedEmptyLegacyUncertain {
                next_relist_at: next,
            })
        } else {
            self.lifecycle.mark_physically_purged(did).await?;
            count("purge-verified");
            Ok(PurgeOutcome::VerifiedPurged)
        }
    }

    /// Every actor with a directory or an open obligation.
    pub async fn collect_all(
        &self,
        actor_store: &ActorStore,
        factory: &BlobstoreFactory,
    ) -> Result<Vec<ActorReport>> {
        self.refuse_in_coexistence()?;
        let mut dids = actor_store.list_dids().await?;
        for obligation in self.lifecycle.open_purge_obligations().await? {
            if !dids.contains(&obligation.did) {
                dids.push(obligation.did);
            }
        }
        dids.sort();
        let mut reports = Vec::new();
        for did in dids {
            let blobstore = factory.blobstore(did.clone());
            match self.collect_actor(actor_store, blobstore, &did).await {
                Ok(report) => reports.push(report),
                Err(error) => {
                    tracing::error!(did, %error, "collector run failed for the actor");
                    reports.push(ActorReport {
                        did,
                        purge: Some(PurgeOutcome::Open {
                            reason: error.to_string(),
                        }),
                        ..ActorReport::default()
                    });
                }
            }
        }
        Ok(reports)
    }
}

/// Weekly listings for the first quarter after the deletion request, then
/// monthly.
pub fn next_relist(requested_at: &str, now: DateTime<Utc>) -> String {
    let weekly_until = from_str_to_utc(requested_at)
        .map(|requested| requested + Duration::days(LEGACY_WEEKLY_DAYS))
        .unwrap_or(now);
    let step = if now < weekly_until {
        Duration::days(7)
    } else {
        Duration::days(30)
    };
    (now + step).to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor_store::blob::insert_blob_work_in;
    use crate::actor_store::blobstore::MemoryBlobStore;
    use crate::background::BackgroundQueue;
    use crate::config::ActorStoreConfig;
    use crate::lifecycle::PurgeObligation;
    use secp256k1::{Keypair, Secp256k1, SecretKey};

    const DID: &str = "did:plc:collectme";
    const CID: &str = "bafkreigqmv2x5m2p6dtg6ny6q2p2ynqpqbqjn3q3nzqcqhmjsbmgfyjjxq";

    struct World {
        _dir: tempfile::TempDir,
        lifecycle: LifecycleStore,
        attempts: AttemptJournal,
        generations: Generations,
        actor_store: ActorStore,
        store: Arc<MemoryBlobStore>,
    }

    fn keypair() -> Keypair {
        Keypair::from_secret_key(
            &Secp256k1::new(),
            &SecretKey::from_slice(&[9u8; 32]).unwrap(),
        )
    }

    async fn world() -> World {
        crate::account_manager::tests::init_env();
        let dir = tempfile::tempdir().unwrap();
        let lifecycle = LifecycleStore::open(dir.path().join("rsky/lifecycle.sqlite"))
            .await
            .unwrap();
        let attempts = AttemptJournal::open(dir.path().join("rsky/blob-attempts.sqlite"), false)
            .await
            .unwrap();
        let generations = Generations::open(dir.path().join("rsky/blob-generations.sqlite"))
            .await
            .unwrap();
        let actor_store = ActorStore::new(
            &ActorStoreConfig {
                directory: dir.path().join("actors").to_str().unwrap().to_owned(),
                cache_size: 10,
            },
            BackgroundQueue::default(),
            lifecycle.clone(),
        );
        actor_store.create(DID, &keypair()).await.unwrap();
        World {
            _dir: dir,
            lifecycle,
            attempts,
            generations,
            actor_store,
            store: Arc::new(MemoryBlobStore::default()),
        }
    }

    impl World {
        fn collector(&self, coexistence: bool) -> Collector {
            Collector {
                lifecycle: self.lifecycle.clone(),
                attempts: self.attempts.clone(),
                generations: self.generations.clone(),
                coexistence,
            }
        }

        fn blobstore(&self) -> Arc<dyn BlobStore> {
            self.store.clone()
        }

        async fn journal(
            &self,
            kind: BlobWorkKind,
            key: &str,
            cid: Option<&str>,
            state: BlobWorkState,
        ) {
            let reader = self
                .actor_store
                .read(DID.to_owned(), self.blobstore())
                .await
                .unwrap();
            let (key, cid) = (key.to_owned(), cid.map(str::to_owned));
            reader
                .blob
                .db
                .run(move |conn| {
                    insert_blob_work_in(
                        conn,
                        kind,
                        &key,
                        cid.as_deref(),
                        state,
                        None,
                        &rsky_common::now(),
                    )?;
                    Ok(())
                })
                .await
                .unwrap();
        }

        async fn rows(&self) -> Vec<BlobWork> {
            self.actor_store
                .read(DID.to_owned(), self.blobstore())
                .await
                .unwrap()
                .blob
                .blob_work()
                .await
                .unwrap()
        }

        async fn set_state(&self, id: i64, state: BlobWorkState) {
            self.actor_store
                .read(DID.to_owned(), self.blobstore())
                .await
                .unwrap()
                .blob
                .set_blob_work_state(id, state)
                .await
                .unwrap();
        }

        async fn reference(&self, cid: &str) {
            let reader = self
                .actor_store
                .read(DID.to_owned(), self.blobstore())
                .await
                .unwrap();
            let cid = cid.to_owned();
            reader
                .blob
                .db
                .run(move |conn| {
                    conn.execute(
                        "INSERT INTO record_blob (\"blobCid\", \"recordUri\") VALUES (?1, ?2)",
                        rusqlite::params![cid, format!("at://{DID}/app.bsky.feed.post/x")],
                    )?;
                    Ok(())
                })
                .await
                .unwrap();
        }

        async fn collect(&self) -> ActorReport {
            self.collector(false)
                .collect_actor(&self.actor_store, self.blobstore(), DID)
                .await
                .unwrap()
        }
    }

    fn stored(store: &MemoryBlobStore, cid: &str) {
        let cid: lexicon_cid::Cid = cid.parse().unwrap();
        futures::executor::block_on(store.put_permanent(cid, b"bytes".to_vec())).unwrap();
    }

    #[tokio::test]
    async fn the_collector_refuses_while_the_store_is_shared() {
        let world = world().await;
        let err = world
            .collector(true)
            .collect_actor(&world.actor_store, world.blobstore(), DID)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("PDS_COEXISTENCE"));
        let factory = BlobstoreFactory::new(
            crate::config::BlobstoreConfig::Disk {
                location: "/nonexistent".into(),
                tmp_location: None,
            },
            aws_config::SdkConfig::builder().build(),
        );
        assert!(world
            .collector(true)
            .collect_all(&world.actor_store, &factory)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn deferred_objects_are_deleted_once_and_referenced_ones_are_kept() {
        let world = world().await;
        stored(&world.store, CID);
        world
            .journal(
                BlobWorkKind::Permanent,
                CID,
                Some(CID),
                BlobWorkState::GcDeferred,
            )
            .await;
        world
            .journal(
                BlobWorkKind::Temp,
                "temp-1",
                Some(CID),
                BlobWorkState::GcDeferred,
            )
            .await;
        world.store.put_named("temp/temp-1", b"t".to_vec());
        world
            .journal(
                BlobWorkKind::Quarantine,
                "quarantined-1",
                None,
                BlobWorkState::GcDeferred,
            )
            .await;
        world
            .store
            .put_named("quarantine/quarantined-1", b"q".to_vec());
        // a second row names content that a record references again
        let other = "bafkreidnxfqnmufnfvjpxb4trzk3elfamwyd3lo4iwt7v3yq6v4rgkpfke";
        stored(&world.store, other);
        world
            .journal(
                BlobWorkKind::Permanent,
                other,
                Some(other),
                BlobWorkState::GcDeferred,
            )
            .await;
        world.reference(other).await;

        let report = world.collect().await;
        assert_eq!(
            (report.checked, report.deleted, report.superseded),
            (4, 3, 1)
        );
        assert_eq!(world.store.stored_cids(), vec![other.to_owned()]);
        assert!(!world.store.has_temp("temp-1"));
        assert_eq!(
            world.store.physical_keys(),
            vec![format!("permanent/{other}")]
        );
        let rows = world.rows().await;
        assert!(rows.iter().all(|row| row.state.is_terminal()));
        let attempts = world.attempts.all(DID).await.unwrap();
        assert_eq!(attempts.len(), 3);
        assert!(attempts
            .iter()
            .all(|a| a.operation == "delete" && !a.is_unresolved()));
        assert_eq!(attempts[0].key, format!("permanent/{CID}"));
        // a second run has nothing left to do
        let again = world.collect().await;
        assert_eq!(again.checked, 0);
    }

    #[tokio::test]
    async fn an_unconfirmed_delete_retires_the_key_and_the_content_moves_to_a_new_generation() {
        let world = world().await;
        stored(&world.store, CID);
        world
            .journal(
                BlobWorkKind::Permanent,
                CID,
                Some(CID),
                BlobWorkState::GcDeferred,
            )
            .await;
        let key = format!("permanent/{CID}");
        world
            .store
            .fail_next_delete(&key, DeleteError::Ambiguous("timeout".into()));

        let report = world.collect().await;
        assert_eq!((report.ambiguous, report.retired), (1, 1));
        let rows = world.rows().await;
        assert_eq!(rows[0].state, BlobWorkState::Retired);
        assert_eq!(world.generations.current(DID, CID).await.unwrap(), 1);
        assert!(world.generations.is_retired(DID, &key).await.unwrap());
        let attempt = world.attempts.latest(DID, &key).await.unwrap().unwrap();
        assert!(attempt.is_unresolved());

        // through the registry the content is unavailable at the retired
        // key and a re-upload lands at the generation key the old delete
        // never named
        let factory = BlobstoreFactory::new(
            crate::config::BlobstoreConfig::Disk {
                location: "/nonexistent".into(),
                tmp_location: None,
            },
            aws_config::SdkConfig::builder().build(),
        )
        .with_attempts(world.attempts.clone())
        .with_generations(world.generations.clone());
        let aware = factory.generation_aware(DID.to_owned(), world.blobstore());
        let cid: lexicon_cid::Cid = CID.parse().unwrap();
        assert!(!aware.has_stored(cid).await.unwrap());
        let temp = aware.put_temp(b"again".to_vec()).await.unwrap();
        aware.make_permanent(temp, cid).await.unwrap();
        assert!(aware.has_stored(cid).await.unwrap());
        assert_eq!(aware.get_bytes(cid).await.unwrap(), b"again");
        assert!(world
            .store
            .physical_keys()
            .contains(&format!("permanent/{CID}.g1")));
        // the stream and copy-only paths follow the generation too
        let streamed = aware
            .get_stream(cid)
            .await
            .unwrap()
            .collect()
            .await
            .unwrap()
            .into_bytes();
        assert_eq!(&streamed[..], b"again");
        let temp = aware.put_temp(b"again".to_vec()).await.unwrap();
        aware.make_permanent_copy_only(temp, cid).await.unwrap();
        aware.put_permanent(cid, b"again".to_vec()).await.unwrap();
        BlobStore::delete(aware.as_ref(), cid).await.unwrap();
        assert!(!aware.has_stored(cid).await.unwrap());
        aware.delete_many(vec![cid]).await.unwrap();

        // an actor snapshot restored from before the retirement changes
        // nothing: the registry, outside the restore set, still knows
        world.set_state(rows[0].id, BlobWorkState::GcDeferred).await;
        stored(&world.store, CID);
        let report = world.collect().await;
        assert_eq!(report.deleted, 1, "{report:?}");
        assert_eq!(
            world
                .attempts
                .latest(DID, &format!("permanent/{CID}.g1"))
                .await
                .unwrap()
                .unwrap()
                .outcome
                .as_deref(),
            Some("succeeded")
        );
        assert_eq!(world.generations.current(DID, CID).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn a_refused_delete_is_retried_and_a_crash_after_issuing_is_resumed() {
        let world = world().await;
        stored(&world.store, CID);
        world
            .journal(
                BlobWorkKind::Permanent,
                CID,
                Some(CID),
                BlobWorkState::GcDeferred,
            )
            .await;
        let key = format!("permanent/{CID}");
        world
            .store
            .fail_next_delete(&key, DeleteError::Definitive("forbidden".into()));
        let report = world.collect().await;
        assert_eq!(report.failed, 1);
        assert_eq!(world.rows().await[0].state, BlobWorkState::Failed);
        let report = world.collect().await;
        assert_eq!(report.deleted, 1);
        assert_eq!(world.rows().await[0].state, BlobWorkState::Done);

        // crash after journaling: the outcome of the recorded attempt decides
        let row = world.rows().await[0].id;
        world.set_state(row, BlobWorkState::Issuing).await;
        let report = world.collect().await;
        assert_eq!(report.deleted, 1);
        world.set_state(row, BlobWorkState::Issuing).await;
        let id = world.attempts.begin(DID, &key, "delete").await.unwrap();
        world
            .attempts
            .resolve(id, AttemptOutcome::Failed("later refusal".into()))
            .await
            .unwrap();
        let report = world.collect().await;
        assert_eq!(report.failed, 1);
        world.set_state(row, BlobWorkState::Issuing).await;
        world.attempts.begin(DID, &key, "delete").await.unwrap();
        let report = world.collect().await;
        assert_eq!((report.ambiguous, report.retired), (1, 1));
        assert_eq!(world.rows().await[0].state, BlobWorkState::Retired);
        // an ambiguous row found on restart is retired as well, once
        world.set_state(row, BlobWorkState::Ambiguous).await;
        let report = world.collect().await;
        assert_eq!(report.retired, 1);
        assert_eq!(world.generations.retirements(DID).await.unwrap().len(), 1);
        // a temp key with an unconfirmed delete stays ambiguous for an operator
        world
            .journal(
                BlobWorkKind::Temp,
                "temp-9",
                None,
                BlobWorkState::GcDeferred,
            )
            .await;
        world.store.put_named("temp/temp-9", b"t".to_vec());
        world
            .store
            .fail_next_delete("temp/temp-9", DeleteError::Ambiguous("lost".into()));
        let report = world.collect().await;
        assert_eq!((report.ambiguous, report.retired), (1, 0));
        assert_eq!(world.rows().await[1].state, BlobWorkState::Ambiguous);
        // a temp row that was issuing when the process died, with no
        // recorded outcome, is ambiguous too and retires nothing
        let temp_row = world.rows().await[1].id;
        world.set_state(temp_row, BlobWorkState::Issuing).await;
        let report = world.collect().await;
        assert_eq!(
            (report.checked, report.ambiguous, report.retired),
            (1, 1, 0)
        );
        assert_eq!(world.rows().await[1].state, BlobWorkState::Ambiguous);
        assert_eq!(world.generations.retirements(DID).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_stale_registry_stops_the_collector() {
        let world = world().await;
        world
            .journal(
                BlobWorkKind::Permanent,
                CID,
                Some(CID),
                BlobWorkState::Retired,
            )
            .await;
        let err = world
            .collector(false)
            .collect_actor(&world.actor_store, world.blobstore(), DID)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("older than the journal"), "{err}");
    }

    #[tokio::test]
    async fn purge_outcomes_follow_the_evidence() {
        let world = world().await;
        let obligation = PurgeObligation {
            did: DID.to_owned(),
            requested_at: rsky_common::now(),
            namespace_prefixes: vec!["permanent/".into(), "temp/".into(), "quarantine/".into()],
            manifest: serde_json::json!({}),
        };
        world
            .lifecycle
            .record_purge_obligation(&obligation)
            .await
            .unwrap();
        stored(&world.store, CID);
        world.store.put_named("temp/left-behind", b"t".to_vec());
        // this process wrote the namespace and no other implementation could
        let write = world
            .attempts
            .begin(DID, "permanent/x", "put")
            .await
            .unwrap();
        world
            .attempts
            .resolve(write, AttemptOutcome::Succeeded)
            .await
            .unwrap();

        // a refusal keeps the obligation open
        world
            .store
            .fail_next_delete("temp/left-behind", DeleteError::Definitive("no".into()));
        let report = world.collect().await;
        assert!(
            matches!(report.purge, Some(PurgeOutcome::Open { ref reason }) if reason.contains("refused")),
            "{report:?}"
        );

        // a delete that was never confirmed leaves the object in the listing
        world
            .store
            .fail_next_delete("temp/left-behind", DeleteError::Ambiguous("lost".into()));
        let report = world.collect().await;
        assert!(
            matches!(report.purge, Some(PurgeOutcome::Open { ref reason }) if reason.contains("remain")),
            "{report:?}"
        );
        assert_eq!(
            world.store.physical_keys(),
            vec!["temp/left-behind".to_owned()]
        );

        // an unconfirmed write keeps it open even when the listing is empty
        let pending = world
            .attempts
            .begin(DID, "permanent/y", "put")
            .await
            .unwrap();
        let report = world.collect().await;
        assert!(
            matches!(report.purge, Some(PurgeOutcome::Open { ref reason }) if reason.contains("never confirmed")),
            "{report:?}"
        );
        assert!(world.store.physical_keys().is_empty());
        world
            .attempts
            .resolve(pending, AttemptOutcome::Failed("rejected".into()))
            .await
            .unwrap();

        // with every write confirmed and no legacy writer, the purge is proven
        let report = world.collect().await;
        assert_eq!(report.purge, Some(PurgeOutcome::VerifiedPurged));
        let progress = world
            .lifecycle
            .purge_progress_of(DID)
            .await
            .unwrap()
            .unwrap();
        assert!(progress.physically_purged_at.is_some());
        assert!(world
            .lifecycle
            .open_purge_obligations()
            .await
            .unwrap()
            .is_empty());
        let report = world.collect().await;
        assert!(report.purge.is_none());
    }

    #[tokio::test]
    async fn a_namespace_another_writer_touched_is_only_observed_empty_and_listed_again() {
        let world = world().await;
        let attempts =
            AttemptJournal::open(world._dir.path().join("rsky/legacy-attempts.sqlite"), true)
                .await
                .unwrap();
        let collector = Collector {
            lifecycle: world.lifecycle.clone(),
            attempts: attempts.clone(),
            generations: world.generations.clone(),
            coexistence: false,
        };
        let obligation = PurgeObligation {
            did: DID.to_owned(),
            requested_at: rsky_common::now(),
            namespace_prefixes: vec![],
            manifest: serde_json::json!({}),
        };
        world
            .lifecycle
            .record_purge_obligation(&obligation)
            .await
            .unwrap();
        // no attempt history at all: unprovable, so observed empty at best
        let report = collector
            .collect_actor(&world.actor_store, world.blobstore(), DID)
            .await
            .unwrap();
        let next = match report.purge {
            Some(PurgeOutcome::ObservedEmptyLegacyUncertain { next_relist_at }) => next_relist_at,
            other => panic!("{other:?}"),
        };
        let progress = world
            .lifecycle
            .purge_progress_of(DID)
            .await
            .unwrap()
            .unwrap();
        assert!(progress.observed_empty_at.is_some());
        assert!(progress.physically_purged_at.is_none());
        assert_eq!(progress.next_relist_at.as_deref(), Some(next.as_str()));
        assert!(
            world
                .lifecycle
                .open_purge_obligations()
                .await
                .unwrap()
                .len()
                == 1
        );
        // before the next listing is due the namespace is skipped
        let report = collector
            .collect_actor(&world.actor_store, world.blobstore(), DID)
            .await
            .unwrap();
        assert_eq!(
            report.purge,
            Some(PurgeOutcome::Skipped {
                until: next.clone()
            })
        );
        // a reappeared object is removed by the next listing
        world
            .lifecycle
            .mark_observed_empty(DID, "2000-01-01T00:00:00.000Z")
            .await
            .unwrap();
        world.store.put_named("permanent/ghost", b"g".to_vec());
        let report = collector
            .collect_actor(&world.actor_store, world.blobstore(), DID)
            .await
            .unwrap();
        assert!(matches!(
            report.purge,
            Some(PurgeOutcome::ObservedEmptyLegacyUncertain { .. })
        ));
        assert!(world.store.physical_keys().is_empty());
        // a namespace this process wrote in coexistence is legacy-uncertain too
        let write = attempts.begin(DID, "permanent/z", "copy").await.unwrap();
        attempts
            .resolve(write, AttemptOutcome::Succeeded)
            .await
            .unwrap();
        world
            .lifecycle
            .mark_observed_empty(DID, "2000-01-01T00:00:00.000Z")
            .await
            .unwrap();
        let report = collector
            .collect_actor(&world.actor_store, world.blobstore(), DID)
            .await
            .unwrap();
        assert!(matches!(
            report.purge,
            Some(PurgeOutcome::ObservedEmptyLegacyUncertain { .. })
        ));
        assert!(
            attempts
                .namespace_of(DID)
                .await
                .unwrap()
                .unwrap()
                .ts_era_writes_possible
        );
    }

    #[tokio::test]
    async fn every_actor_and_obligation_is_visited() {
        let world = world().await;
        let obligation = PurgeObligation {
            did: "did:plc:gone".to_owned(),
            requested_at: rsky_common::now(),
            namespace_prefixes: vec![],
            manifest: serde_json::json!({}),
        };
        world
            .lifecycle
            .record_purge_obligation(&obligation)
            .await
            .unwrap();
        let factory = BlobstoreFactory::new(
            crate::config::BlobstoreConfig::Disk {
                location: world
                    ._dir
                    .path()
                    .join("blocks")
                    .to_str()
                    .unwrap()
                    .to_owned(),
                tmp_location: None,
            },
            aws_config::SdkConfig::builder().build(),
        )
        .with_attempts(world.attempts.clone())
        .with_generations(world.generations.clone());
        // entries that are not actor directories are skipped
        let actors = world._dir.path().join("actors");
        std::fs::create_dir_all(actors.join("reserved_keys")).unwrap();
        std::fs::write(actors.join("stray"), b"").unwrap();
        let shard = world
            .actor_store
            .get_location(DID)
            .unwrap()
            .directory
            .parent()
            .unwrap()
            .to_path_buf();
        std::fs::write(shard.join("notes"), b"").unwrap();
        std::fs::create_dir_all(shard.join("not-a-did")).unwrap();
        let reports = world
            .collector(false)
            .collect_all(&world.actor_store, &factory)
            .await
            .unwrap();
        let dids: Vec<&str> = reports.iter().map(|report| report.did.as_str()).collect();
        assert_eq!(dids, vec![DID, "did:plc:gone"]);
        assert!(reports[0].purge.is_none());
        assert!(matches!(
            reports[1].purge,
            Some(PurgeOutcome::ObservedEmptyLegacyUncertain { .. })
        ));
        assert!(factory.generations().is_some() && factory.attempts().is_some());

        // an actor whose run fails is reported open, and the others still run
        world
            .journal(
                BlobWorkKind::Permanent,
                CID,
                Some(CID),
                BlobWorkState::Retired,
            )
            .await;
        let reports = world
            .collector(false)
            .collect_all(&world.actor_store, &factory)
            .await
            .unwrap();
        assert_eq!(reports.len(), 2);
        assert!(
            matches!(reports[0].purge, Some(PurgeOutcome::Open { ref reason }) if reason.contains("older than the journal")),
            "{:?}",
            reports[0]
        );
        assert!(matches!(
            reports[1].purge,
            Some(PurgeOutcome::Skipped { .. })
        ));

        // a store directory that does not exist yet holds no actors
        let empty = ActorStore::new(
            &ActorStoreConfig {
                directory: world
                    ._dir
                    .path()
                    .join("nowhere")
                    .to_str()
                    .unwrap()
                    .to_owned(),
                cache_size: 1,
            },
            BackgroundQueue::default(),
            world.lifecycle.clone(),
        );
        assert!(empty.list_dids().await.unwrap().is_empty());
    }

    #[test]
    fn relisting_is_weekly_for_a_quarter_then_monthly() {
        let requested = "2026-01-01T00:00:00.000Z";
        let early = next_relist(
            requested,
            from_str_to_utc("2026-02-01T00:00:00.000Z").unwrap(),
        );
        assert_eq!(early, "2026-02-08T00:00:00.000Z");
        let late = next_relist(
            requested,
            from_str_to_utc("2026-06-01T00:00:00.000Z").unwrap(),
        );
        assert_eq!(late, "2026-07-01T00:00:00.000Z");
        let now = Utc::now();
        assert!(next_relist("not a date", now) > now.to_rfc3339_opts(SecondsFormat::Millis, true));
    }
}
