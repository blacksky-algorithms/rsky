//! Account deletion as a durable, resumable sequence of steps, and the
//! tombstones that keep a deleted account from being written again.
//!
//! The reference PDS deletes an account in three moves: remove the account
//! rows, sequence the deletion event, then destroy the actor store. This
//! module keeps that order and records each move in its own journal, so a
//! crash between moves is resumed at startup instead of leaving an account
//! half deleted, and every write path can refuse a DID whose deletion is in
//! progress.

use crate::account_manager::helpers::account::AccountStatus;
use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobStore;
use crate::actor_store::ActorStore;
use crate::db::migrator::{migrate_to_latest, Migration, MigrationSet};
use crate::db::sqlite::Db;
use crate::sequencer::events::CommitEvt;
use crate::SharedSequencer;
use anyhow::{bail, Result};
use rusqlite::{params, OptionalExtension};
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, RwLock};

const LIFECYCLE_MIGRATIONS: &[Migration] = &[
    Migration {
        name: "001",
        sql: "CREATE TABLE tombstone (\
            did TEXT PRIMARY KEY, \
            \"requestedAt\" TEXT NOT NULL, \
            \"accountSeq\" INTEGER, \
            \"logicallyDeletedAt\" TEXT\
          );\
          CREATE TABLE purge_obligation (\
            did TEXT PRIMARY KEY, \
            \"requestedAt\" TEXT NOT NULL, \
            \"namespacePrefixes\" TEXT NOT NULL, \
            manifest TEXT NOT NULL, \
            \"observedEmptyAt\" TEXT, \
            \"physicallyPurgedAt\" TEXT\
          );",
    },
    // A write marks its actor here before it commits, so a restart knows
    // which stores may hold undelivered publication intents or unfinished
    // blob work without opening every store on disk.
    Migration {
        name: "002",
        sql: "CREATE TABLE pending_work (\
            did TEXT PRIMARY KEY, \
            \"markedAt\" TEXT NOT NULL\
          );",
    },
    // What this process has published, served, restored, or pruned for an
    // actor: the evidence a downstream reconciliation needs to know how far
    // this actor's history reaches, kept outside the stores a restore
    // rewinds.
    Migration {
        name: "003",
        sql: "CREATE TABLE frontier_watermark (\
            did TEXT PRIMARY KEY, \
            \"maxRev\" TEXT, \
            \"exposedMaxRev\" TEXT, \
            \"creationSeq\" INTEGER, \
            \"preDeletionMaxRev\" TEXT\
          );\
          CREATE TABLE restore_event (\
            id INTEGER PRIMARY KEY AUTOINCREMENT, \
            did TEXT NOT NULL, \
            at TEXT NOT NULL, \
            \"resultingRev\" TEXT NOT NULL\
          );\
          CREATE INDEX restore_event_did_idx ON restore_event (did);\
          CREATE TABLE revision_floor (\
            did TEXT PRIMARY KEY, \
            rev TEXT NOT NULL\
          );",
    },
];

const LIFECYCLE_MIGRATION_SET: MigrationSet = MigrationSet {
    shared: &[],
    local: LIFECYCLE_MIGRATIONS,
    legacy: None,
};

/// The DIDs whose deletion has begun and not yet completed. Shared with the
/// actor store so no write is admitted for them.
pub type Tombstones = Arc<RwLock<HashSet<String>>>;

/// A write was attempted on an account whose deletion is in progress.
#[derive(Debug, thiserror::Error, PartialEq)]
#[error("Account has been deleted: {0}")]
pub struct AccountDeleting(pub String);

#[derive(Debug, Clone, PartialEq)]
pub struct Tombstone {
    pub did: String,
    pub requested_at: String,
    pub account_seq: Option<i64>,
    pub logically_deleted_at: Option<String>,
}

/// The revisions this process has published, served, and pruned for an
/// actor. Revisions are TIDs, which order as strings.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FrontierWatermark {
    pub did: String,
    /// The highest revision of any event this process published.
    pub max_rev: Option<String>,
    /// The highest revision this process ever served or accepted by import.
    pub exposed_max_rev: Option<String>,
    /// The sequence number of the creation commit, when this process
    /// created the repository.
    pub creation_seq: Option<i64>,
    /// The highest published revision at the moment this process pruned
    /// the actor's history for a deletion.
    pub pre_deletion_max_rev: Option<String>,
}

fn max_rev(current: Option<String>, candidate: &str) -> String {
    match current {
        Some(current) if current.as_str() >= candidate => current,
        _ => candidate.to_owned(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PurgeObligation {
    pub did: String,
    pub requested_at: String,
    pub namespace_prefixes: Vec<String>,
    pub manifest: serde_json::Value,
}

/// The journal of deletions, purge obligations, and actors with work
/// outstanding, kept outside every actor store and written with full
/// synchronous durability.
#[derive(Clone)]
pub struct LifecycleStore {
    db: Db,
    tombstoned: Tombstones,
}

impl LifecycleStore {
    pub async fn open(location: impl AsRef<Path>) -> Result<Self> {
        let parent = location
            .as_ref()
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| Path::new(".").to_path_buf());
        tokio::fs::create_dir_all(&parent).await?;
        let db = Db::open(location)?;
        db.run(|conn| Ok(conn.pragma_update(None, "synchronous", "FULL")?))
            .await?;
        migrate_to_latest(&db, LIFECYCLE_MIGRATION_SET).await?;
        let store = Self {
            db,
            tombstoned: Arc::new(RwLock::new(HashSet::new())),
        };
        let open: HashSet<String> = store
            .open_tombstones()
            .await?
            .into_iter()
            .map(|tombstone| tombstone.did)
            .collect();
        *store.tombstoned.write().expect("tombstone set poisoned") = open;
        Ok(store)
    }

    pub fn tombstones(&self) -> Tombstones {
        self.tombstoned.clone()
    }

    /// Records, before a write commits, that `did` may have undelivered
    /// intents or unfinished blob work afterwards.
    pub async fn mark_pending_work(&self, did: &str) -> Result<()> {
        crate::metrics::METRICS.control_journal_write("pending_work");
        let did = did.to_owned();
        let now = rsky_common::now();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO pending_work (did, \"markedAt\") VALUES (?1, ?2) \
                     ON CONFLICT (did) DO NOTHING",
                    params![did, now],
                )?;
                Ok(())
            })
            .await
    }

    /// Forgets the mark once the actor's intents are delivered and its
    /// blob work is terminal.
    pub async fn clear_pending_work(&self, did: &str) -> Result<()> {
        crate::metrics::METRICS.control_journal_write("pending_work");
        let did = did.to_owned();
        self.db
            .run(move |conn| {
                conn.execute("DELETE FROM pending_work WHERE did = ?1", params![did])?;
                Ok(())
            })
            .await
    }

    pub async fn frontier_watermark(&self, did: &str) -> Result<Option<FrontierWatermark>> {
        let did = did.to_owned();
        self.db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT did, \"maxRev\", \"exposedMaxRev\", \"creationSeq\", \
                         \"preDeletionMaxRev\" FROM frontier_watermark WHERE did = ?1",
                        [&did],
                        |row| {
                            Ok(FrontierWatermark {
                                did: row.get(0)?,
                                max_rev: row.get(1)?,
                                exposed_max_rev: row.get(2)?,
                                creation_seq: row.get(3)?,
                                pre_deletion_max_rev: row.get(4)?,
                            })
                        },
                    )
                    .optional()?)
            })
            .await
    }

    async fn update_watermark(
        &self,
        did: &str,
        update: impl Fn(FrontierWatermark) -> FrontierWatermark + Send + 'static,
    ) -> Result<()> {
        let did = did.to_owned();
        self.db
            .tx(move |tx| {
                let current = tx
                    .query_row(
                        "SELECT \"maxRev\", \"exposedMaxRev\", \"creationSeq\", \
                         \"preDeletionMaxRev\" FROM frontier_watermark WHERE did = ?1",
                        [&did],
                        |row| {
                            Ok(FrontierWatermark {
                                did: did.clone(),
                                max_rev: row.get(0)?,
                                exposed_max_rev: row.get(1)?,
                                creation_seq: row.get(2)?,
                                pre_deletion_max_rev: row.get(3)?,
                            })
                        },
                    )
                    .optional()?
                    .unwrap_or_else(|| FrontierWatermark {
                        did: did.clone(),
                        ..Default::default()
                    });
                let next = update(current);
                tx.execute(
                    "INSERT INTO frontier_watermark \
                     (did, \"maxRev\", \"exposedMaxRev\", \"creationSeq\", \"preDeletionMaxRev\") \
                     VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT (did) DO UPDATE SET \
                     \"maxRev\" = excluded.\"maxRev\", \
                     \"exposedMaxRev\" = excluded.\"exposedMaxRev\", \
                     \"creationSeq\" = excluded.\"creationSeq\", \
                     \"preDeletionMaxRev\" = excluded.\"preDeletionMaxRev\"",
                    params![
                        did,
                        next.max_rev,
                        next.exposed_max_rev,
                        next.creation_seq,
                        next.pre_deletion_max_rev
                    ],
                )?;
                Ok(())
            })
            .await
    }

    /// Records a commit this process published; `creation` marks the
    /// repository's first commit when this process created it.
    pub async fn record_publication(
        &self,
        did: &str,
        rev: &str,
        seq: i64,
        creation: bool,
    ) -> Result<()> {
        crate::metrics::METRICS.control_journal_write("frontier_watermark");
        let rev = rev.to_owned();
        self.update_watermark(did, move |mut mark| {
            mark.max_rev = Some(max_rev(mark.max_rev.take(), &rev));
            if creation && mark.creation_seq.is_none() {
                mark.creation_seq = Some(seq);
            }
            mark
        })
        .await
    }

    /// Records, before the first response byte, the highest revision a
    /// read is about to serve or an import has accepted.
    pub async fn record_exposure(&self, did: &str, rev: &str) -> Result<()> {
        crate::metrics::METRICS.control_journal_write("frontier_watermark");
        let rev = rev.to_owned();
        self.update_watermark(did, move |mut mark| {
            mark.exposed_max_rev = Some(max_rev(mark.exposed_max_rev.take(), &rev));
            mark
        })
        .await
    }

    /// Records the highest published revision before a deletion prunes the
    /// actor's history; a later re-creation is complete only above it.
    pub async fn record_pre_deletion_max(&self, did: &str, rev: Option<&str>) -> Result<()> {
        crate::metrics::METRICS.control_journal_write("frontier_watermark");
        let rev = rev.map(str::to_owned);
        self.update_watermark(did, move |mut mark| {
            let candidate = [mark.max_rev.clone(), rev.clone()]
                .into_iter()
                .flatten()
                .max();
            mark.pre_deletion_max_rev = match (mark.pre_deletion_max_rev.take(), candidate) {
                (Some(current), Some(candidate)) => Some(max_rev(Some(current), &candidate)),
                (current, candidate) => current.or(candidate),
            };
            mark
        })
        .await
    }

    /// Records that the actor's store was restored or rewritten to
    /// `resulting_rev` outside the ordinary write path.
    pub async fn record_restore_event(&self, did: &str, resulting_rev: &str) -> Result<()> {
        crate::metrics::METRICS.control_journal_write("restore_event");
        let (did, resulting_rev) = (did.to_owned(), resulting_rev.to_owned());
        let now = rsky_common::now();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO restore_event (did, at, \"resultingRev\") VALUES (?1, ?2, ?3)",
                    params![did, now, resulting_rev],
                )?;
                Ok(())
            })
            .await
    }

    pub async fn restore_event_count(&self, did: &str) -> Result<i64> {
        let did = did.to_owned();
        self.db
            .run(move |conn| {
                Ok(conn.query_row(
                    "SELECT count(*) FROM restore_event WHERE did = ?1",
                    [&did],
                    |row| row.get(0),
                )?)
            })
            .await
    }

    /// Every later commit for the actor must exceed `rev`. The floor only
    /// ever rises.
    pub async fn raise_revision_floor(&self, did: &str, rev: &str) -> Result<()> {
        crate::metrics::METRICS.control_journal_write("revision_floor");
        let (did, rev) = (did.to_owned(), rev.to_owned());
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO revision_floor (did, rev) VALUES (?1, ?2) \
                     ON CONFLICT (did) DO UPDATE SET rev = max(rev, excluded.rev)",
                    params![did, rev],
                )?;
                Ok(())
            })
            .await
    }

    pub async fn revision_floor(&self, did: &str) -> Result<Option<String>> {
        let did = did.to_owned();
        self.db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT rev FROM revision_floor WHERE did = ?1",
                        [&did],
                        |row| row.get(0),
                    )
                    .optional()?)
            })
            .await
    }

    pub async fn pending_work(&self) -> Result<Vec<String>> {
        self.db
            .run(|conn| {
                let mut stmt =
                    conn.prepare("SELECT did FROM pending_work ORDER BY \"markedAt\", did")?;
                let dids = stmt
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(dids)
            })
            .await
    }

    /// Whether writes for `did` must be refused because its deletion is in
    /// progress.
    pub fn blocks_writes(&self, did: &str) -> bool {
        self.tombstoned
            .read()
            .expect("tombstone set poisoned")
            .contains(did)
    }

    /// Step 1 of a deletion: from this moment the DID is non-writable.
    pub async fn tombstone(&self, did: &str) -> Result<()> {
        crate::metrics::METRICS.control_journal_write("tombstone");
        let now = rsky_common::now();
        let did_owned = did.to_owned();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO tombstone (did, \"requestedAt\") VALUES (?1, ?2) \
                     ON CONFLICT (did) DO NOTHING",
                    params![did_owned, now],
                )?;
                Ok(())
            })
            .await?;
        self.tombstoned
            .write()
            .expect("tombstone set poisoned")
            .insert(did.to_owned());
        Ok(())
    }

    pub async fn record_deletion_seq(&self, did: &str, seq: i64) -> Result<()> {
        crate::metrics::METRICS.control_journal_write("tombstone");
        let did = did.to_owned();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE tombstone SET \"accountSeq\" = ?1 WHERE did = ?2",
                    params![seq, did],
                )?;
                Ok(())
            })
            .await
    }

    pub async fn record_purge_obligation(&self, obligation: &PurgeObligation) -> Result<()> {
        crate::metrics::METRICS.control_journal_write("purge_obligation");
        let obligation = obligation.clone();
        let prefixes = serde_json::to_string(&obligation.namespace_prefixes)
            .expect("a list of strings serializes");
        let manifest =
            serde_json::to_string(&obligation.manifest).expect("a JSON value serializes");
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO purge_obligation \
                     (did, \"requestedAt\", \"namespacePrefixes\", manifest) \
                     VALUES (?1, ?2, ?3, ?4) ON CONFLICT (did) DO NOTHING",
                    params![obligation.did, obligation.requested_at, prefixes, manifest],
                )?;
                Ok(())
            })
            .await
    }

    /// The final step: the account is gone logically, and the DID may be
    /// created again; the purge obligation stays until the objects are.
    pub async fn mark_logically_deleted(&self, did: &str) -> Result<()> {
        crate::metrics::METRICS.control_journal_write("tombstone");
        let now = rsky_common::now();
        let did_owned = did.to_owned();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE tombstone SET \"logicallyDeletedAt\" = ?1 WHERE did = ?2",
                    params![now, did_owned],
                )?;
                Ok(())
            })
            .await?;
        self.tombstoned
            .write()
            .expect("tombstone set poisoned")
            .remove(did);
        Ok(())
    }

    /// Forgets a completed deletion when the DID is created again.
    pub async fn clear_tombstone(&self, did: &str) -> Result<()> {
        crate::metrics::METRICS.control_journal_write("tombstone");
        let did = did.to_owned();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM tombstone WHERE did = ?1 AND \"logicallyDeletedAt\" IS NOT NULL",
                    params![did],
                )?;
                Ok(())
            })
            .await
    }

    pub async fn tombstone_of(&self, did: &str) -> Result<Option<Tombstone>> {
        let did = did.to_owned();
        self.db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT did, \"requestedAt\", \"accountSeq\", \"logicallyDeletedAt\" \
                         FROM tombstone WHERE did = ?1",
                        params![did],
                        tombstone_from_row,
                    )
                    .optional()?)
            })
            .await
    }

    pub async fn open_tombstones(&self) -> Result<Vec<Tombstone>> {
        self.db
            .run(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT did, \"requestedAt\", \"accountSeq\", \"logicallyDeletedAt\" \
                     FROM tombstone WHERE \"logicallyDeletedAt\" IS NULL ORDER BY \"requestedAt\"",
                )?;
                let rows = stmt
                    .query_map([], tombstone_from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
    }

    pub async fn purge_obligation_of(&self, did: &str) -> Result<Option<PurgeObligation>> {
        let did = did.to_owned();
        self.db
            .run(move |conn| {
                let row: Option<(String, String, String, String)> = conn
                    .query_row(
                        "SELECT did, \"requestedAt\", \"namespacePrefixes\", manifest \
                         FROM purge_obligation WHERE did = ?1",
                        params![did],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                    )
                    .optional()?;
                row.map(|(did, requested_at, prefixes, manifest)| {
                    Ok(PurgeObligation {
                        did,
                        requested_at,
                        namespace_prefixes: serde_json::from_str(&prefixes)?,
                        manifest: serde_json::from_str(&manifest)?,
                    })
                })
                .transpose()
            })
            .await
    }
}

fn tombstone_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Tombstone> {
    Ok(Tombstone {
        did: row.get(0)?,
        requested_at: row.get(1)?,
        account_seq: row.get(2)?,
        logically_deleted_at: row.get(3)?,
    })
}

/// The moves of a deletion, in the order the reference PDS makes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DeletionStep {
    Tombstone,
    AccountRows,
    Sequence,
    PurgeObligation,
    Unlink,
    Complete,
}

/// Everything a deletion touches.
pub struct DeletionContext<'a> {
    pub lifecycle: &'a LifecycleStore,
    pub account_manager: &'a AccountManager,
    pub sequencer: &'a SharedSequencer,
    pub actor_store: &'a ActorStore,
    /// The account's blob store, when its objects are to be deleted now.
    /// `None` while another implementation may still serve them, in which
    /// case a purge obligation is recorded instead.
    pub blobstore: Option<Arc<dyn BlobStore>>,
}

/// Deletes `did`, resuming from whatever earlier moves already completed.
/// `stop_after` ends the deletion early, which tests use to stand in for a
/// crash between moves.
pub async fn delete_account(
    ctx: &DeletionContext<'_>,
    did: &str,
    stop_after: Option<DeletionStep>,
) -> Result<DeletionStep> {
    let stopped = |step: DeletionStep| stop_after == Some(step);

    ctx.lifecycle.tombstone(did).await?;
    if stopped(DeletionStep::Tombstone) {
        return Ok(DeletionStep::Tombstone);
    }

    // the rows are the source of truth; removing them first means no
    // credential or lookup can find the account from here on
    ctx.account_manager.delete_account(did).await?;
    if stopped(DeletionStep::AccountRows) {
        return Ok(DeletionStep::AccountRows);
    }

    let already_sequenced = ctx
        .lifecycle
        .tombstone_of(did)
        .await?
        .and_then(|tombstone| tombstone.account_seq);
    if already_sequenced.is_none() {
        let mut lock = ctx.sequencer.sequencer.write().await;
        // the highest revision ever published survives the pruning below;
        // a re-creation of the DID is only complete above it
        let published_max = lock
            .rows_for_did_after(did, 0)
            .await?
            .into_iter()
            .filter(|row| row.event_type == "append")
            .filter_map(|row| rsky_common::cbor_to_struct::<CommitEvt>(row.event).ok())
            .map(|evt| evt.rev)
            .max();
        ctx.lifecycle
            .record_pre_deletion_max(did, published_max.as_deref())
            .await?;
        let seq = lock
            .sequence_account_evt(did.to_owned(), AccountStatus::Deleted)
            .await?;
        lock.delete_all_for_user(did, Some(vec![seq])).await?;
        ctx.lifecycle.record_deletion_seq(did, seq).await?;
    }
    if stopped(DeletionStep::Sequence) {
        return Ok(DeletionStep::Sequence);
    }

    match &ctx.blobstore {
        None => {
            let obligation = purge_obligation_for(ctx, did).await?;
            ctx.lifecycle.record_purge_obligation(&obligation).await?;
        }
        Some(blobstore) => {
            ctx.actor_store.delete_blobs(did, blobstore.clone()).await?;
        }
    }
    if stopped(DeletionStep::PurgeObligation) {
        return Ok(DeletionStep::PurgeObligation);
    }

    ctx.actor_store.unlink(did).await?;
    if stopped(DeletionStep::Unlink) {
        return Ok(DeletionStep::Unlink);
    }

    ctx.lifecycle.mark_logically_deleted(did).await?;
    Ok(DeletionStep::Complete)
}

/// Records what the account left in blob storage: the namespaces are
/// authoritative, the manifest is a snapshot for the collector's audit.
async fn purge_obligation_for(ctx: &DeletionContext<'_>, did: &str) -> Result<PurgeObligation> {
    let blobs: Vec<String> = if ctx.actor_store.exists(did).await? {
        ctx.actor_store.blob_cids(did).await?
    } else {
        vec![]
    };
    Ok(PurgeObligation {
        did: did.to_owned(),
        requested_at: rsky_common::now(),
        namespace_prefixes: vec![
            format!("blocks/{did}/"),
            format!("tmp/{did}/"),
            format!("quarantine/{did}/"),
        ],
        manifest: serde_json::json!({ "blobs": blobs }),
    })
}

/// Finishes every deletion a previous process left incomplete.
pub async fn resume_deletions(ctx: &DeletionContext<'_>) -> Result<Vec<String>> {
    let mut resumed = Vec::new();
    for tombstone in ctx.lifecycle.open_tombstones().await? {
        tracing::warn!(did = %tombstone.did, "resuming an incomplete account deletion");
        delete_account(ctx, &tombstone.did, None).await?;
        resumed.push(tombstone.did);
    }
    Ok(resumed)
}

/// Refuses a write for a DID whose deletion is in progress.
pub fn assert_not_deleting(tombstones: &Tombstones, did: &str) -> Result<()> {
    if tombstones
        .read()
        .expect("tombstone set poisoned")
        .contains(did)
    {
        bail!(AccountDeleting(did.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_manager::{AccountManager, CreateAccountOpts};
    use crate::actor_store::blobstore::MemoryBlobStore;
    use crate::background::BackgroundQueue;
    use crate::config::ActorStoreConfig;
    use crate::crawlers::Crawlers;
    use crate::sequencer::Sequencer;
    use lexicon_cid::Cid;
    use secp256k1::{Keypair, Secp256k1, SecretKey};
    use std::str::FromStr;

    const DID: &str = "did:plc:lifecycle";
    const TEST_CID: &str = "bafkreibjfgx2gprinfvicegelk5kosd6y2frmqpqzwqkg7usac74l3t2v4";

    struct World {
        _dir: tempfile::TempDir,
        lifecycle: LifecycleStore,
        account_manager: AccountManager,
        sequencer: SharedSequencer,
        actor_store: ActorStore,
    }

    impl World {
        fn ctx(&self, blobstore: Option<Arc<dyn BlobStore>>) -> DeletionContext<'_> {
            DeletionContext {
                lifecycle: &self.lifecycle,
                account_manager: &self.account_manager,
                sequencer: &self.sequencer,
                actor_store: &self.actor_store,
                blobstore,
            }
        }

        async fn account_rows(&self) -> i64 {
            self.account_manager
                .get_account(
                    DID,
                    Some(
                        crate::account_manager::helpers::account::AvailabilityFlags {
                            include_deactivated: Some(true),
                            include_taken_down: Some(true),
                        },
                    ),
                )
                .await
                .unwrap()
                .map(|_| 1)
                .unwrap_or(0)
        }

        async fn seq_rows(&self) -> Vec<String> {
            let lock = self.sequencer.sequencer.read().await;
            lock.request_seq_range(crate::sequencer::RequestSeqRangeOpts {
                earliest_seq: None,
                latest_seq: None,
                earliest_time: None,
                limit: None,
            })
            .await
            .unwrap()
            .into_iter()
            .map(|evt| format!("{:?}", std::mem::discriminant(&evt)))
            .collect()
        }
    }

    async fn world() -> World {
        crate::account_manager::tests::init_env();
        let dir = tempfile::tempdir().unwrap();
        let lifecycle = LifecycleStore::open(dir.path().join("rsky/lifecycle.sqlite"))
            .await
            .unwrap();
        let account_db =
            crate::account_manager::db::get_migrated_db(dir.path().join("account.sqlite"))
                .await
                .unwrap();
        let account_manager = AccountManager::new(account_db);
        let sequencer_db =
            crate::sequencer::db::get_migrated_db(dir.path().join("sequencer.sqlite"))
                .await
                .unwrap();
        let sequencer = SharedSequencer {
            sequencer: tokio::sync::RwLock::new(Sequencer::new(
                sequencer_db,
                Crawlers::new("pds.test".to_owned(), vec![]),
                None,
            )),
        };
        let actor_store = ActorStore::new(
            &ActorStoreConfig {
                directory: dir.path().join("actors").to_str().unwrap().to_owned(),
                cache_size: 10,
            },
            BackgroundQueue::default(),
            lifecycle.clone(),
        );
        World {
            _dir: dir,
            lifecycle,
            account_manager,
            sequencer,
            actor_store,
        }
    }

    fn keypair() -> Keypair {
        Keypair::from_secret_key(
            &Secp256k1::new(),
            &SecretKey::from_slice(&[7u8; 32]).unwrap(),
        )
    }

    #[tokio::test]
    async fn frontier_watermarks_only_ever_rise() {
        let world = world().await;
        assert!(world
            .lifecycle
            .frontier_watermark(DID)
            .await
            .unwrap()
            .is_none());
        world
            .lifecycle
            .record_publication(DID, "3lfixtureaa2b", 7, true)
            .await
            .unwrap();
        world
            .lifecycle
            .record_publication(DID, "3lfixtureaa2a", 8, true)
            .await
            .unwrap();
        let mark = world
            .lifecycle
            .frontier_watermark(DID)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(mark.max_rev.as_deref(), Some("3lfixtureaa2b"));
        assert_eq!(mark.creation_seq, Some(7), "the first creation wins");
        assert_eq!(mark.exposed_max_rev, None);

        world
            .lifecycle
            .record_exposure(DID, "3lfixtureaa2c")
            .await
            .unwrap();
        world
            .lifecycle
            .record_exposure(DID, "3lfixtureaa2a")
            .await
            .unwrap();
        let mark = world
            .lifecycle
            .frontier_watermark(DID)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(mark.exposed_max_rev.as_deref(), Some("3lfixtureaa2c"));

        // the pre-deletion maximum takes the published maximum and the
        // caller's candidate, and never drops on a later call
        world
            .lifecycle
            .record_pre_deletion_max(DID, None)
            .await
            .unwrap();
        let mark = world
            .lifecycle
            .frontier_watermark(DID)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(mark.pre_deletion_max_rev.as_deref(), Some("3lfixtureaa2b"));
        world
            .lifecycle
            .record_pre_deletion_max(DID, Some("3lfixtureaa2z"))
            .await
            .unwrap();
        world
            .lifecycle
            .record_pre_deletion_max(DID, Some("3lfixtureaa2a"))
            .await
            .unwrap();
        let mark = world
            .lifecycle
            .frontier_watermark(DID)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(mark.pre_deletion_max_rev.as_deref(), Some("3lfixtureaa2z"));
        world
            .lifecycle
            .record_pre_deletion_max("did:plc:fresh", None)
            .await
            .unwrap();
        assert_eq!(
            world
                .lifecycle
                .frontier_watermark("did:plc:fresh")
                .await
                .unwrap()
                .unwrap()
                .pre_deletion_max_rev,
            None
        );

        assert_eq!(world.lifecycle.restore_event_count(DID).await.unwrap(), 0);
        world
            .lifecycle
            .record_restore_event(DID, "3lfixtureaa2a")
            .await
            .unwrap();
        assert_eq!(world.lifecycle.restore_event_count(DID).await.unwrap(), 1);

        assert!(world.lifecycle.revision_floor(DID).await.unwrap().is_none());
        world
            .lifecycle
            .raise_revision_floor(DID, "3lfixtureaa2m")
            .await
            .unwrap();
        world
            .lifecycle
            .raise_revision_floor(DID, "3lfixtureaa2c")
            .await
            .unwrap();
        assert_eq!(
            world
                .lifecycle
                .revision_floor(DID)
                .await
                .unwrap()
                .as_deref(),
            Some("3lfixtureaa2m")
        );
    }

    #[tokio::test]
    async fn deletion_records_the_published_maximum_before_pruning() {
        let world = world().await;
        create_account(&world).await;
        {
            let mut lock = world.sequencer.sequencer.write().await;
            let mut evt = crate::sequencer::events::TypedCommitEvt::default().evt;
            evt.repo = DID.to_owned();
            evt.rev = "3lfixtureaa2q".to_owned();
            lock.sequence_evt(crate::models::models::RepoSeq::new(
                DID.to_owned(),
                "append".to_owned(),
                rsky_common::struct_to_cbor(&evt).unwrap(),
                rsky_common::now(),
            ))
            .await
            .unwrap();
            lock.sequence_evt(crate::models::models::RepoSeq::new(
                DID.to_owned(),
                "append".to_owned(),
                vec![0xff],
                rsky_common::now(),
            ))
            .await
            .unwrap();
        }
        delete_account(&world.ctx(None), DID, None).await.unwrap();
        let mark = world
            .lifecycle
            .frontier_watermark(DID)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(mark.pre_deletion_max_rev.as_deref(), Some("3lfixtureaa2q"));
        assert_eq!(
            world.seq_rows().await.len(),
            1,
            "only the deletion survives"
        );
    }

    #[tokio::test]
    async fn pending_work_marks_are_idempotent_and_ordered() {
        let world = world().await;
        world
            .lifecycle
            .mark_pending_work("did:plc:b")
            .await
            .unwrap();
        world
            .lifecycle
            .mark_pending_work("did:plc:a")
            .await
            .unwrap();
        world
            .lifecycle
            .mark_pending_work("did:plc:b")
            .await
            .unwrap();
        let mut marked = world.lifecycle.pending_work().await.unwrap();
        marked.sort();
        assert_eq!(marked, ["did:plc:a", "did:plc:b"]);
        world
            .lifecycle
            .clear_pending_work("did:plc:b")
            .await
            .unwrap();
        world
            .lifecycle
            .clear_pending_work("did:plc:b")
            .await
            .unwrap();
        assert_eq!(world.lifecycle.pending_work().await.unwrap(), ["did:plc:a"]);
    }

    async fn create_account(world: &World) {
        world
            .account_manager
            .create_account(CreateAccountOpts {
                did: DID.to_owned(),
                handle: "lifecycle.test".to_owned(),
                email: Some("lifecycle@example.com".to_owned()),
                password: Some("password123".to_owned()),
                repo_cid: Cid::from_str(TEST_CID).unwrap(),
                repo_rev: "3jzfcijpj2z2a".to_owned(),
                invite_code: None,
                deactivated: None,
            })
            .await
            .unwrap();
        world.actor_store.create(DID, &keypair()).await.unwrap();
        let mut lock = world.sequencer.sequencer.write().await;
        lock.sequence_account_evt(DID.to_owned(), AccountStatus::Active)
            .await
            .unwrap();
        lock.sequence_identity_evt(DID.to_owned(), Some("lifecycle.test".to_owned()))
            .await
            .unwrap();
    }

    /// A bare file name opens in the working directory.
    #[tokio::test]
    async fn opens_a_bare_file_name() {
        let name = format!("lifecycle-test-{}.sqlite", std::process::id());
        let store = LifecycleStore::open(&name).await.unwrap();
        assert!(!store.blocks_writes(DID));
        drop(store);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{name}{suffix}"));
        }
    }

    #[tokio::test]
    async fn journal_round_trip_and_reload() {
        let world = world().await;
        let store = &world.lifecycle;
        assert!(!store.blocks_writes(DID));
        store.tombstone(DID).await.unwrap();
        store.tombstone(DID).await.unwrap();
        assert!(store.blocks_writes(DID));
        let tombstone = store.tombstone_of(DID).await.unwrap().unwrap();
        assert_eq!(tombstone.account_seq, None);
        assert_eq!(tombstone.logically_deleted_at, None);
        store.record_deletion_seq(DID, 42).await.unwrap();
        assert_eq!(
            store.tombstone_of(DID).await.unwrap().unwrap().account_seq,
            Some(42)
        );
        let obligation = PurgeObligation {
            did: DID.to_owned(),
            requested_at: rsky_common::now(),
            namespace_prefixes: vec![format!("blocks/{DID}/")],
            manifest: serde_json::json!({"blobs": ["bafy"]}),
        };
        store.record_purge_obligation(&obligation).await.unwrap();
        store.record_purge_obligation(&obligation).await.unwrap();
        assert_eq!(
            store.purge_obligation_of(DID).await.unwrap(),
            Some(obligation)
        );
        assert_eq!(
            store.purge_obligation_of("did:plc:other").await.unwrap(),
            None
        );
        assert_eq!(store.open_tombstones().await.unwrap().len(), 1);

        // an open tombstone survives a restart
        let path = world._dir.path().join("rsky/lifecycle.sqlite");
        let reopened = LifecycleStore::open(&path).await.unwrap();
        assert!(reopened.blocks_writes(DID));

        store.mark_logically_deleted(DID).await.unwrap();
        assert!(!store.blocks_writes(DID));
        assert!(store.open_tombstones().await.unwrap().is_empty());
        assert!(store
            .tombstone_of(DID)
            .await
            .unwrap()
            .unwrap()
            .logically_deleted_at
            .is_some());
        store.clear_tombstone(DID).await.unwrap();
        assert_eq!(store.tombstone_of(DID).await.unwrap(), None);
        // the obligation outlives the tombstone
        assert!(store.purge_obligation_of(DID).await.unwrap().is_some());

        let tombstones = store.tombstones();
        tombstones.write().unwrap().insert("did:plc:x".to_owned());
        let err = assert_not_deleting(&tombstones, "did:plc:x").unwrap_err();
        assert_eq!(err.to_string(), "Account has been deleted: did:plc:x");
        assert!(assert_not_deleting(&tombstones, DID).is_ok());
    }

    /// A deletion interrupted after any move is resumed to the same end
    /// state, and the account is unwritable from the first move on.
    #[tokio::test]
    async fn deletion_resumes_after_every_move() {
        for stop in [
            DeletionStep::Tombstone,
            DeletionStep::AccountRows,
            DeletionStep::Sequence,
            DeletionStep::PurgeObligation,
            DeletionStep::Unlink,
        ] {
            let world = world().await;
            create_account(&world).await;
            assert_eq!(world.seq_rows().await.len(), 2);

            let reached = delete_account(&world.ctx(None), DID, Some(stop))
                .await
                .unwrap();
            assert_eq!(reached, stop);
            assert!(world.lifecycle.blocks_writes(DID), "{stop:?}");
            let blobstore: Arc<dyn BlobStore> = Arc::new(MemoryBlobStore::default());
            let err = world
                .actor_store
                .transact(DID.to_owned(), blobstore.clone())
                .await
                .map(drop)
                .unwrap_err();
            assert!(err.downcast_ref::<AccountDeleting>().is_some(), "{stop:?}");
            assert!(world.actor_store.create(DID, &keypair()).await.is_err());
            if stop == DeletionStep::Tombstone {
                assert_eq!(world.account_rows().await, 1);
            } else {
                assert_eq!(world.account_rows().await, 0);
            }
            if stop >= DeletionStep::Sequence {
                assert_eq!(world.seq_rows().await.len(), 1);
                assert!(world
                    .lifecycle
                    .tombstone_of(DID)
                    .await
                    .unwrap()
                    .unwrap()
                    .account_seq
                    .is_some());
            }
            if stop >= DeletionStep::PurgeObligation {
                assert!(world
                    .lifecycle
                    .purge_obligation_of(DID)
                    .await
                    .unwrap()
                    .is_some());
            }
            assert_eq!(
                world.actor_store.exists(DID).await.unwrap(),
                stop < DeletionStep::Unlink
            );

            // a restart finishes the job without repeating the deletion event
            let resumed = resume_deletions(&world.ctx(None)).await.unwrap();
            assert_eq!(resumed, vec![DID.to_owned()]);
            assert!(!world.lifecycle.blocks_writes(DID));
            assert_eq!(world.seq_rows().await.len(), 1);
            assert!(!world.actor_store.exists(DID).await.unwrap());
            let obligation = world
                .lifecycle
                .purge_obligation_of(DID)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                obligation.namespace_prefixes,
                vec![
                    format!("blocks/{DID}/"),
                    format!("tmp/{DID}/"),
                    format!("quarantine/{DID}/")
                ]
            );
            assert_eq!(obligation.manifest, serde_json::json!({"blobs": []}));
            assert!(resume_deletions(&world.ctx(None)).await.unwrap().is_empty());
            // the DID can be created again once the deletion is complete
            world.actor_store.create(DID, &keypair()).await.unwrap();
        }
    }

    /// Outside coexistence the objects are deleted now and no obligation is
    /// recorded.
    #[tokio::test]
    async fn deletion_with_a_blobstore_deletes_objects_directly() {
        let world = world().await;
        create_account(&world).await;
        let blobstore: Arc<dyn BlobStore> = Arc::new(MemoryBlobStore::default());
        let reached = delete_account(&world.ctx(Some(blobstore)), DID, None)
            .await
            .unwrap();
        assert_eq!(reached, DeletionStep::Complete);
        assert!(world
            .lifecycle
            .purge_obligation_of(DID)
            .await
            .unwrap()
            .is_none());
        assert!(!world.actor_store.exists(DID).await.unwrap());
        assert_eq!(world.seq_rows().await.len(), 1);
        // deleting an account that was already deleted is harmless
        assert_eq!(
            delete_account(&world.ctx(None), DID, None).await.unwrap(),
            DeletionStep::Complete
        );
    }
}
