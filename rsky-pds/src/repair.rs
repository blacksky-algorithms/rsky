//! Repairs and quarantines, journaled so every step survives a crash and
//! never overwrites what a client wrote in between.
//!
//! A consumed revision is never re-delivered: every repair lands as a new
//! commit on the repository. A repair runs only for an actor whose
//! allowlist entry names it, only after client writes have drained, and
//! only while holding the actor's maintenance slot. Each commit it makes is
//! recorded in the store with the step it belongs to, so a restart resumes
//! from the last commit that actually landed; each step swaps against the
//! exact root the previous step produced, so any other root, a client's
//! included, ends the repair as superseded rather than rebased.

use crate::actor_store::blobstore::BlobStore;
use crate::actor_store::repo::sql_repo::ConcurrentWriteError;
use crate::actor_store::ActorStore;
use crate::db::migrator::{migrate_to_latest, Migration, MigrationSet};
use crate::db::sqlite::{Db, Synchronous};
use crate::lifecycle::LifecycleStore;
use crate::locks::LockDir;
use crate::publication::publish_pending;
use crate::repo::prepare::{prepare_create, prepare_delete, PrepareCreateOpts, PrepareDeleteOpts};
use crate::SharedSequencer;
use anyhow::{bail, Context, Result};
use lexicon_cid::Cid;
use rsky_repo::types::PreparedWrite;
use rsky_syntax::aturi::AtUri;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

const MIGRATIONS: &[Migration] = &[Migration {
    name: "001",
    sql: "CREATE TABLE repair (\
            id TEXT PRIMARY KEY, \
            did TEXT NOT NULL, \
            kind TEXT NOT NULL, \
            state TEXT NOT NULL, \
            payload TEXT NOT NULL, \
            \"createdAt\" TEXT NOT NULL, \
            \"doneAt\" TEXT, \
            outcome TEXT\
          );\
          CREATE INDEX repair_did_state_idx ON repair (did, state);\
          CREATE TABLE quarantine (\
            seq INTEGER PRIMARY KEY, \
            did TEXT NOT NULL, \
            kind TEXT NOT NULL, \
            state TEXT NOT NULL, \
            \"linkedRepairs\" TEXT NOT NULL, \
            \"localState\" TEXT NOT NULL, \
            \"externalState\" TEXT NOT NULL, \
            \"externalRequired\" INTEGER NOT NULL, \
            \"openedAt\" TEXT NOT NULL, \
            \"closedAt\" TEXT, \
            justification TEXT\
          );\
          CREATE TABLE quarantine_step (\
            seq INTEGER NOT NULL, \
            step TEXT NOT NULL, \
            \"doneAt\" TEXT NOT NULL, \
            PRIMARY KEY (seq, step)\
          );",
}];

const MIGRATION_SET: MigrationSet = MigrationSet {
    shared: &[],
    local: MIGRATIONS,
    legacy: None,
};

/// What a repair does, with everything it needs captured durably before
/// its first commit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RepairKind {
    /// Delete and re-create a record at the same key and value, so
    /// consumers receive it again at a new revision.
    Republish {
        uri: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        captured: Option<CapturedRecord>,
    },
    /// A commit with no record changes above `boundary`, moving a repository
    /// past a revision consumers have already seen.
    EmptyCommit { boundary: String },
    /// Create a record consumers still hold and delete it again, so they
    /// receive an explicit deletion at a new revision.
    PhantomDelete {
        uri: String,
        value: serde_json::Value,
    },
}

impl RepairKind {
    pub fn name(&self) -> &'static str {
        match self {
            RepairKind::Republish { .. } => "republish",
            RepairKind::EmptyCommit { .. } => "empty-commit",
            RepairKind::PhantomDelete { .. } => "phantom-delete",
        }
    }
}

/// The record a republish will re-create, exactly as it was.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapturedRecord {
    pub cid: String,
    pub value: serde_json::Value,
    /// The root the repair was planned against; its first step swaps
    /// against it.
    pub root: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairState {
    Pending,
    Running,
    Done,
    /// A client changed the repository between steps; its state stands.
    ClientSuperseded,
    Failed,
}

impl RepairState {
    pub fn as_str(self) -> &'static str {
        match self {
            RepairState::Pending => "pending",
            RepairState::Running => "running",
            RepairState::Done => "done",
            RepairState::ClientSuperseded => "client-superseded",
            RepairState::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        Ok(match value {
            "pending" => RepairState::Pending,
            "running" => RepairState::Running,
            "done" => RepairState::Done,
            "client-superseded" => RepairState::ClientSuperseded,
            "failed" => RepairState::Failed,
            other => bail!("unknown repair state: {other}"),
        })
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            RepairState::Done | RepairState::ClientSuperseded | RepairState::Failed
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Repair {
    pub id: String,
    pub did: String,
    pub kind: RepairKind,
    pub state: String,
    pub created_at: String,
    pub done_at: Option<String>,
    pub outcome: Option<String>,
}

/// The moves of a quarantine, each persisted when done.
pub const QUARANTINE_STEPS: [&str; 3] = ["supersede-intent", "invalidate-row", "open-repairs"];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Quarantine {
    pub seq: i64,
    pub did: String,
    pub kind: String,
    pub state: String,
    pub linked_repairs: Vec<String>,
    pub local_state: String,
    pub external_state: String,
    pub external_required: bool,
    pub opened_at: String,
    pub closed_at: Option<String>,
    pub justification: Option<String>,
    pub steps_done: Vec<String>,
}

#[derive(Clone)]
pub struct RepairStore {
    db: Db,
}

impl RepairStore {
    pub async fn open(location: impl AsRef<Path>) -> Result<Self> {
        if let Some(parent) = location
            .as_ref()
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            tokio::fs::create_dir_all(parent).await?;
        }
        let db = Db::open_with(location, Synchronous::Full)?;
        migrate_to_latest(&db, MIGRATION_SET).await?;
        Ok(RepairStore { db })
    }

    pub async fn create(&self, id: &str, did: &str, kind: &RepairKind) -> Result<()> {
        let (id, did) = (id.to_owned(), did.to_owned());
        let name = kind.name().to_owned();
        let payload = serde_json::to_string(kind)?;
        let now = rsky_common::now();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO repair (id, did, kind, state, payload, \"createdAt\") \
                     VALUES (?1, ?2, ?3, 'pending', ?4, ?5)",
                    params![id, did, name, payload, now],
                )?;
                Ok(())
            })
            .await
    }

    pub async fn get(&self, id: &str) -> Result<Option<Repair>> {
        let id = id.to_owned();
        self.db
            .run(move |conn| {
                conn.query_row(
                    "SELECT id, did, payload, state, \"createdAt\", \"doneAt\", outcome \
                     FROM repair WHERE id = ?1",
                    [&id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, Option<String>>(5)?,
                            row.get::<_, Option<String>>(6)?,
                        ))
                    },
                )
                .optional()?
                .map(|(id, did, payload, state, created_at, done_at, outcome)| {
                    Ok(Repair {
                        id,
                        did,
                        kind: serde_json::from_str(&payload)?,
                        state,
                        created_at,
                        done_at,
                        outcome,
                    })
                })
                .transpose()
            })
            .await
    }

    async fn update_payload(&self, id: &str, kind: &RepairKind) -> Result<()> {
        let id = id.to_owned();
        let payload = serde_json::to_string(kind)?;
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE repair SET payload = ?1 WHERE id = ?2",
                    params![payload, id],
                )?;
                Ok(())
            })
            .await
    }

    async fn set_state(&self, id: &str, state: RepairState, outcome: Option<String>) -> Result<()> {
        let id = id.to_owned();
        let done_at = state.is_terminal().then(rsky_common::now);
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE repair SET state = ?1, outcome = ?2, \"doneAt\" = ?3 WHERE id = ?4",
                    params![state.as_str(), outcome, done_at, id],
                )?;
                Ok(())
            })
            .await
    }

    /// Repairs for `did` that are not terminal.
    pub async fn pending_for(&self, did: &str) -> Result<Vec<String>> {
        let did = did.to_owned();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id FROM repair WHERE did = ?1 \
                     AND state NOT IN ('done', 'client-superseded', 'failed') ORDER BY \"createdAt\"",
                )?;
                let ids = stmt
                    .query_map([&did], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(ids)
            })
            .await
    }

    pub async fn open_quarantine(
        &self,
        seq: i64,
        did: &str,
        kind: &str,
        linked_repairs: &[String],
    ) -> Result<()> {
        let (did, kind) = (did.to_owned(), kind.to_owned());
        let linked = serde_json::to_string(linked_repairs)?;
        let now = rsky_common::now();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO quarantine (seq, did, kind, state, \"linkedRepairs\", \
                     \"localState\", \"externalState\", \"externalRequired\", \"openedAt\") \
                     VALUES (?1, ?2, ?3, 'opened', ?4, 'pending', 'pending', 1, ?5) \
                     ON CONFLICT (seq) DO NOTHING",
                    params![seq, did, kind, linked, now],
                )?;
                Ok(())
            })
            .await
    }

    pub async fn quarantine(&self, seq: i64) -> Result<Option<Quarantine>> {
        self.db
            .run(move |conn| {
                let row = conn
                    .query_row(
                        "SELECT seq, did, kind, state, \"linkedRepairs\", \"localState\", \
                         \"externalState\", \"externalRequired\", \"openedAt\", \"closedAt\", \
                         justification FROM quarantine WHERE seq = ?1",
                        [seq],
                        |row| {
                            Ok(Quarantine {
                                seq: row.get(0)?,
                                did: row.get(1)?,
                                kind: row.get(2)?,
                                state: row.get(3)?,
                                linked_repairs: vec![],
                                local_state: row.get(5)?,
                                external_state: row.get(6)?,
                                external_required: row.get::<_, i64>(7)? != 0,
                                opened_at: row.get(8)?,
                                closed_at: row.get(9)?,
                                justification: row.get(10)?,
                                steps_done: vec![],
                            })
                            .map(|q| (q, row.get::<_, String>(4)))
                        },
                    )
                    .optional()?;
                let Some((mut quarantine, linked)) = row else {
                    return Ok(None);
                };
                quarantine.linked_repairs = serde_json::from_str(&linked?)?;
                let mut stmt =
                    conn.prepare("SELECT step FROM quarantine_step WHERE seq = ?1 ORDER BY rowid")?;
                quarantine.steps_done = stmt
                    .query_map([seq], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Some(quarantine))
            })
            .await
    }

    async fn mark_quarantine_step(&self, seq: i64, step: &str) -> Result<()> {
        let step = step.to_owned();
        let now = rsky_common::now();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO quarantine_step (seq, step, \"doneAt\") VALUES (?1, ?2, ?3) \
                     ON CONFLICT (seq, step) DO NOTHING",
                    params![seq, step, now],
                )?;
                Ok(())
            })
            .await
    }

    /// The operator's own reconciliation of the local index ran clean.
    pub async fn mark_local_reconciled(&self, seq: i64) -> Result<()> {
        self.db
            .run(move |conn| {
                let changed = conn.execute(
                    "UPDATE quarantine SET \"localState\" = 'reconciled' WHERE seq = ?1",
                    [seq],
                )?;
                if changed == 0 {
                    bail!("no quarantine for seq {seq}");
                }
                Ok(())
            })
            .await
    }

    /// Open quarantines for `did`.
    pub async fn open_quarantines_for(&self, did: &str) -> Result<Vec<i64>> {
        let did = did.to_owned();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT seq FROM quarantine WHERE did = ?1 AND state <> 'closed' ORDER BY seq",
                )?;
                let seqs = stmt
                    .query_map([&did], |row| row.get::<_, i64>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(seqs)
            })
            .await
    }
}

/// Everything a repair or quarantine touches.
pub struct RepairContext<'a> {
    pub actor_store: &'a ActorStore,
    pub sequencer: &'a SharedSequencer,
    pub lifecycle: &'a LifecycleStore,
    pub repairs: &'a RepairStore,
    pub lock_dir: &'a LockDir,
    pub blobstore: Arc<dyn BlobStore>,
}

/// Where a repair stops early; tests stop after a step to stand in for a
/// crash there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StopAfterStep(pub i64);

fn is_swap_failure(err: &anyhow::Error) -> bool {
    err.downcast_ref::<ConcurrentWriteError>().is_some()
        || err.to_string().contains("BadCommitSwapError")
}

async fn client_quiescent(ctx: &RepairContext<'_>, did: &str) -> Result<bool> {
    if ctx.actor_store.inflight_mutations(did) > 0 {
        return Ok(false);
    }
    let pending = ctx
        .actor_store
        .read(did.to_owned(), ctx.blobstore.clone())
        .await?
        .pending_intents()
        .await?;
    Ok(pending.is_empty())
}

/// Waits, holding no lock, until no client write is in flight and nothing
/// is left to publish; then takes the maintenance slot and checks again.
async fn acquire_maintenance(
    ctx: &RepairContext<'_>,
    did: &str,
    timeout: Duration,
) -> Result<crate::locks::FileLock> {
    let started = Instant::now();
    loop {
        while !client_quiescent(ctx, did).await? {
            if started.elapsed() >= timeout {
                bail!("{did} still has client writes in flight after {timeout:?}");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        let slot = ctx.lock_dir.maintenance_slot_within(did, remaining).await?;
        // a write that slipped in before the slot was taken means waiting
        // again; the slot is released at the end of this iteration
        if client_quiescent(ctx, did).await? {
            return Ok(slot);
        }
    }
}

fn record_from_value(value: &serde_json::Value) -> Result<rsky_repo::types::RepoRecord> {
    Ok(serde_json::from_value(value.clone())?)
}

/// One step's write, swapping against the root the previous step left.
async fn commit_step(
    ctx: &RepairContext<'_>,
    repair_id: &str,
    did: &str,
    step_no: i64,
    expected_root: &str,
    writes: Vec<PreparedWrite>,
) -> Result<Result<(String, String), String>> {
    let mut txn = ctx
        .actor_store
        .transact_maintenance(did.to_owned(), ctx.blobstore.clone(), repair_id)
        .await?;
    let expected = Cid::from_str(expected_root)?;
    match txn
        .process_repair_step(writes, Some(expected), repair_id, step_no)
        .await
    {
        Ok(commit) => {
            drop(txn);
            publish_pending(ctx.actor_store, ctx.sequencer, did, None).await?;
            Ok(Ok((
                commit.commit_data.cid.to_string(),
                commit.commit_data.rev,
            )))
        }
        Err(err) if is_swap_failure(&err) => Ok(Err(format!(
            "step {step_no} found a root other than {expected_root}: {err}"
        ))),
        Err(err) => Err(err),
    }
}

async fn current_root(ctx: &RepairContext<'_>, did: &str) -> Result<String> {
    let reader = ctx
        .actor_store
        .read(did.to_owned(), ctx.blobstore.clone())
        .await?;
    let root = reader.storage.read().await.get_root_detailed().await?;
    Ok(root.cid.to_string())
}

/// Runs a repair to a terminal state, resuming from whatever commits it
/// already made. `stop_after` ends the run after that step, as a crash
/// there would.
pub async fn run_repair(
    ctx: &RepairContext<'_>,
    id: &str,
    timeout: Duration,
    stop_after: Option<StopAfterStep>,
) -> Result<Repair> {
    let Some(repair) = ctx.repairs.get(id).await? else {
        bail!("no repair {id}")
    };
    if RepairState::parse(&repair.state)?.is_terminal() {
        return Ok(repair);
    }
    let did = repair.did.clone();
    ctx.actor_store
        .admission
        .admit_maintenance(&did, id)
        .with_context(|| format!("repair {id} is not the maintenance workflow for {did}"))?;
    let _slot = acquire_maintenance(ctx, &did, timeout).await?;
    ctx.repairs
        .set_state(id, RepairState::Running, None)
        .await?;

    let reader = ctx
        .actor_store
        .read(did.clone(), ctx.blobstore.clone())
        .await?;
    let done_steps = reader.repair_steps(id).await?;
    let mut kind = repair.kind.clone();
    let mut expected_root = match &kind {
        RepairKind::Republish {
            captured: Some(captured),
            ..
        } => captured.root.clone(),
        _ => current_root(ctx, &did).await?,
    };

    // republish captures the record before touching anything
    if let RepairKind::Republish { uri, captured } = &mut kind {
        if captured.is_none() {
            let at_uri: AtUri = uri.clone().try_into()?;
            let mut reader = ctx
                .actor_store
                .read(did.clone(), ctx.blobstore.clone())
                .await?;
            let Some(record) = reader.record.get_record(&at_uri, None, Some(true)).await? else {
                let outcome = format!("{uri} does not exist; nothing to republish");
                ctx.repairs
                    .set_state(id, RepairState::Failed, Some(outcome))
                    .await?;
                return Ok(ctx.repairs.get(id).await?.expect("repair exists"));
            };
            *captured = Some(CapturedRecord {
                cid: record.cid,
                value: serde_json::to_value(&record.value)?,
                root: expected_root.clone(),
            });
            ctx.repairs.update_payload(id, &kind).await?;
        }
    }

    let plan: Vec<Vec<PreparedWrite>> = match &kind {
        RepairKind::Republish { uri, captured } => {
            let captured = captured.as_ref().context("the record was captured above")?;
            let at_uri: AtUri = uri.clone().try_into()?;
            let delete = prepare_delete(PrepareDeleteOpts {
                did: did.clone(),
                collection: at_uri.get_collection(),
                rkey: at_uri.get_rkey(),
                swap_cid: None,
            })?;
            let create = prepare_create(PrepareCreateOpts {
                did: did.clone(),
                collection: at_uri.get_collection(),
                rkey: Some(at_uri.get_rkey()),
                swap_cid: None,
                record: record_from_value(&captured.value)?,
                validate: Some(false),
            })
            .await?;
            vec![
                vec![PreparedWrite::Delete(delete)],
                vec![PreparedWrite::Create(create)],
            ]
        }
        RepairKind::EmptyCommit { boundary } => {
            ctx.lifecycle.raise_revision_floor(&did, boundary).await?;
            vec![vec![]]
        }
        RepairKind::PhantomDelete { uri, value } => {
            let at_uri: AtUri = uri.clone().try_into()?;
            let create = prepare_create(PrepareCreateOpts {
                did: did.clone(),
                collection: at_uri.get_collection(),
                rkey: Some(at_uri.get_rkey()),
                swap_cid: None,
                record: record_from_value(value)?,
                validate: Some(false),
            })
            .await?;
            let delete = prepare_delete(PrepareDeleteOpts {
                did: did.clone(),
                collection: at_uri.get_collection(),
                rkey: at_uri.get_rkey(),
                swap_cid: None,
            })?;
            vec![
                vec![PreparedWrite::Create(create)],
                vec![PreparedWrite::Delete(delete)],
            ]
        }
    };

    let mut last_rev = None;
    for (index, writes) in plan.into_iter().enumerate() {
        let step_no = index as i64 + 1;
        if let Some(done) = done_steps.iter().find(|step| step.step_no == step_no) {
            // the commit landed before a crash; continue from its root
            expected_root = done.cid.clone();
            last_rev = Some(done.rev.clone());
            continue;
        }
        match commit_step(ctx, id, &did, step_no, &expected_root, writes).await? {
            Ok((cid, rev)) => {
                expected_root = cid;
                last_rev = Some(rev);
            }
            Err(outcome) => {
                ctx.repairs
                    .set_state(id, RepairState::ClientSuperseded, Some(outcome))
                    .await?;
                return Ok(ctx.repairs.get(id).await?.expect("repair exists"));
            }
        }
        if stop_after == Some(StopAfterStep(step_no)) {
            return Ok(ctx.repairs.get(id).await?.expect("repair exists"));
        }
    }

    // the last step swapped against the previous step's root, so the
    // repository holds exactly what the plan wrote
    let outcome = format!(
        "{} committed at {}",
        kind.name(),
        last_rev.unwrap_or_default()
    );
    ctx.repairs
        .set_state(id, RepairState::Done, Some(outcome))
        .await?;
    Ok(ctx.repairs.get(id).await?.expect("repair exists"))
}

/// Opens a quarantine for one sequenced event and performs its moves in
/// order, each idempotent and recorded when done: the intent that produced
/// the row is superseded, the row is invalidated, and the linked repairs
/// are noted. Rerunning finishes whatever a crash left.
pub async fn quarantine_seq(
    ctx: &RepairContext<'_>,
    seq: i64,
    did: &str,
    kind: &str,
    linked_repairs: &[String],
) -> Result<Quarantine> {
    for id in linked_repairs {
        if ctx.repairs.get(id).await?.is_none() {
            bail!("linked repair {id} does not exist");
        }
    }
    ctx.repairs
        .open_quarantine(seq, did, kind, linked_repairs)
        .await?;
    let done = ctx
        .repairs
        .quarantine(seq)
        .await?
        .map(|q| q.steps_done)
        .unwrap_or_default();
    if !done.iter().any(|step| step == "supersede-intent") {
        if ctx.actor_store.exists(did).await? {
            let db = ctx.actor_store.write_db(did).await?;
            db.run(move |conn| {
                conn.execute(
                    "UPDATE publish_intent SET state = 'superseded' WHERE seq = ?1",
                    [seq],
                )?;
                Ok(())
            })
            .await?;
        }
        ctx.repairs
            .mark_quarantine_step(seq, "supersede-intent")
            .await?;
    }
    if !done.iter().any(|step| step == "invalidate-row") {
        ctx.sequencer.sequencer.read().await.invalidate(seq).await?;
        ctx.repairs
            .mark_quarantine_step(seq, "invalidate-row")
            .await?;
    }
    if !done.iter().any(|step| step == "open-repairs") {
        ctx.repairs
            .mark_quarantine_step(seq, "open-repairs")
            .await?;
    }
    Ok(ctx
        .repairs
        .quarantine(seq)
        .await?
        .expect("quarantine exists"))
}

/// How the outside world was satisfied about a quarantined event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalOutcome {
    Verified,
    Accepted,
}

/// Closes a quarantine: every linked repair is terminal, the local index
/// was reconciled, and every affected consumer was verified or the gap
/// explicitly accepted. Local reconciliation alone never closes one.
pub async fn close_quarantine(
    ctx: &RepairContext<'_>,
    seq: i64,
    external: ExternalOutcome,
    justification: Option<&str>,
) -> Result<Quarantine> {
    let Some(quarantine) = ctx.repairs.quarantine(seq).await? else {
        bail!("no quarantine for seq {seq}")
    };
    if quarantine.state == "closed" {
        return Ok(quarantine);
    }
    for id in &quarantine.linked_repairs {
        let repair = ctx.repairs.get(id).await?;
        let terminal = repair
            .as_ref()
            .map(|repair| RepairState::parse(&repair.state).map(RepairState::is_terminal))
            .transpose()?
            .unwrap_or(false);
        if !terminal {
            bail!("linked repair {id} is not finished");
        }
    }
    if quarantine.local_state != "reconciled" {
        bail!("the local index has not been reconciled for seq {seq}");
    }
    if external == ExternalOutcome::Accepted && justification.is_none_or(str::is_empty) {
        bail!("accepting an external gap needs a justification");
    }
    let external_state = match external {
        ExternalOutcome::Verified => "verified",
        ExternalOutcome::Accepted => "accepted",
    };
    let justification = justification.map(str::to_owned);
    let now = rsky_common::now();
    ctx.repairs
        .db
        .run(move |conn| {
            conn.execute(
                "UPDATE quarantine SET state = 'closed', \"externalState\" = ?1, \
                 justification = ?2, \"closedAt\" = ?3 WHERE seq = ?4",
                params![external_state, justification, now, seq],
            )?;
            Ok(())
        })
        .await?;
    Ok(ctx
        .repairs
        .quarantine(seq)
        .await?
        .expect("quarantine exists"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor_store::blobstore::MemoryBlobStore;
    use crate::admission::Admission;
    use crate::background::BackgroundQueue;
    use crate::config::ActorStoreConfig;
    use crate::crawlers::Crawlers;
    use crate::sequencer::Sequencer;
    use rsky_repo::types::{PreparedCreateOrUpdate, WriteOpAction};
    use secp256k1::{Keypair, Secp256k1, SecretKey};

    const DID: &str = "did:plc:repair";
    const URI: &str = "at://did:plc:repair/app.bsky.feed.post/3lfixtureaa2a";

    struct World {
        dir: tempfile::TempDir,
        actor_store: ActorStore,
        sequencer: SharedSequencer,
        lifecycle: LifecycleStore,
        repairs: RepairStore,
        lock_dir: LockDir,
        blobstore: Arc<MemoryBlobStore>,
    }

    impl World {
        fn ctx(&self) -> RepairContext<'_> {
            RepairContext {
                actor_store: &self.actor_store,
                sequencer: &self.sequencer,
                lifecycle: &self.lifecycle,
                repairs: &self.repairs,
                lock_dir: &self.lock_dir,
                blobstore: self.blobstore.clone(),
            }
        }

        /// Names `workflow` as the actor's maintenance workflow.
        fn fence(&self, workflow: &str) {
            std::fs::write(
                self.dir.path().join("write-allowlist.toml"),
                format!(
                    "version = 1\ndefault = \"active\"\n[entries]\n\"{DID}\" = {{ state = \"maintenance\", workflow_id = \"{workflow}\" }}\n"
                ),
            )
            .unwrap();
            std::fs::File::open(self.dir.path().join("write-allowlist.toml"))
                .unwrap()
                .set_modified(std::time::SystemTime::now() + Duration::from_secs(30))
                .unwrap();
            self.actor_store.admission.reload().unwrap();
        }

        async fn record_cid(&self) -> Option<String> {
            let mut reader = self
                .actor_store
                .read(DID.to_owned(), self.blobstore.clone())
                .await
                .unwrap();
            let uri: AtUri = URI.to_owned().try_into().unwrap();
            reader
                .record
                .get_record(&uri, None, Some(true))
                .await
                .unwrap()
                .map(|record| record.cid)
        }

        async fn event_types(&self) -> Vec<String> {
            self.sequencer
                .sequencer
                .read()
                .await
                .rows_for_did_after(DID, 0)
                .await
                .unwrap()
                .into_iter()
                .map(|row| row.event_type)
                .collect()
        }
    }

    fn post(text: &str) -> PreparedWrite {
        let record: rsky_repo::types::RepoRecord = serde_json::from_value(serde_json::json!({
            "$type": "app.bsky.feed.post",
            "text": text,
            "createdAt": "2023-01-01T00:00:00.000Z",
        }))
        .unwrap();
        let cid = rsky_common::ipld::cid_for_cbor(&record).unwrap();
        PreparedWrite::Create(PreparedCreateOrUpdate {
            action: WriteOpAction::Create,
            uri: URI.to_owned(),
            cid,
            swap_cid: None,
            record,
            blobs: vec![],
        })
    }

    async fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let allowlist = dir.path().join("write-allowlist.toml");
        std::fs::write(&allowlist, "version = 1\ndefault = \"active\"\n").unwrap();
        let lifecycle = LifecycleStore::open(dir.path().join("rsky/lifecycle.sqlite"))
            .await
            .unwrap();
        let lock_dir = LockDir::new(dir.path().join("rsky/locks")).unwrap();
        let actor_store = ActorStore::new(
            &ActorStoreConfig {
                directory: dir.path().join("actors").to_str().unwrap().to_owned(),
                cache_size: 10,
            },
            BackgroundQueue::default(),
            lifecycle.clone(),
        )
        .with_admission(Arc::new(Admission::from_file(&allowlist).unwrap()))
        .with_lock_dir(lock_dir.clone());
        let sequencer = SharedSequencer {
            sequencer: tokio::sync::RwLock::new(Sequencer::new(
                crate::sequencer::db::get_migrated_db(dir.path().join("sequencer.sqlite"))
                    .await
                    .unwrap(),
                Crawlers::new("pds.test".to_owned(), vec![]),
                None,
            )),
        };
        let repairs = RepairStore::open(dir.path().join("rsky/repair.sqlite"))
            .await
            .unwrap();
        let world = World {
            dir,
            actor_store,
            sequencer,
            lifecycle,
            repairs,
            lock_dir,
            blobstore: Arc::new(MemoryBlobStore::default()),
        };
        let keypair = Keypair::from_secret_key(
            &Secp256k1::new(),
            &SecretKey::from_slice(&[11u8; 32]).unwrap(),
        );
        world.actor_store.create(DID, &keypair).await.unwrap();
        let mut txn = world
            .actor_store
            .transact(DID.to_owned(), world.blobstore.clone())
            .await
            .unwrap();
        txn.create_repo(vec![], true).await.unwrap();
        txn.process_writes(vec![post("original")], None)
            .await
            .unwrap();
        drop(txn);
        publish_pending(&world.actor_store, &world.sequencer, DID, None)
            .await
            .unwrap();
        world
    }

    fn republish() -> RepairKind {
        RepairKind::Republish {
            uri: URI.to_owned(),
            captured: None,
        }
    }

    #[tokio::test]
    async fn a_republish_lands_the_same_record_at_a_new_revision() {
        let world = world().await;
        let before = world.record_cid().await.unwrap();
        let events_before = world.event_types().await.len();
        world.repairs.create("r1", DID, &republish()).await.unwrap();

        // not the actor's maintenance workflow: refused before any lock
        let err = run_repair(&world.ctx(), "r1", Duration::from_secs(1), None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not the maintenance workflow"));
        world.fence("r1");

        let done = run_repair(&world.ctx(), "r1", Duration::from_secs(5), None)
            .await
            .unwrap();
        assert_eq!(done.state, "done", "{done:?}");
        assert!(done.outcome.unwrap().contains("republish committed"));
        assert!(done.done_at.is_some());
        assert_eq!(world.record_cid().await.unwrap(), before);
        // two new commits reached the sequencer: the delete and the create
        assert_eq!(world.event_types().await.len(), events_before + 2);
        // the steps are recorded in the store, in order
        let reader = world
            .actor_store
            .read(DID.to_owned(), world.blobstore.clone())
            .await
            .unwrap();
        let steps = reader.repair_steps("r1").await.unwrap();
        assert_eq!(steps.iter().map(|s| s.step_no).collect::<Vec<_>>(), [1, 2]);
        // running a finished repair changes nothing
        let again = run_repair(&world.ctx(), "r1", Duration::from_secs(1), None)
            .await
            .unwrap();
        assert_eq!(again.state, "done");
        assert!(world.repairs.pending_for(DID).await.unwrap().is_empty());
        assert!(
            run_repair(&world.ctx(), "nope", Duration::from_secs(1), None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_crash_between_steps_resumes_to_the_same_record() {
        let world = world().await;
        let before = world.record_cid().await.unwrap();
        world.repairs.create("r2", DID, &republish()).await.unwrap();
        world.fence("r2");
        let stopped = run_repair(
            &world.ctx(),
            "r2",
            Duration::from_secs(5),
            Some(StopAfterStep(1)),
        )
        .await
        .unwrap();
        assert_eq!(stopped.state, "running");
        assert_eq!(
            world.record_cid().await,
            None,
            "deleted, not yet re-created"
        );
        let captured = match &stopped.kind {
            RepairKind::Republish { captured, .. } => captured.clone().unwrap(),
            _ => unreachable!(),
        };
        assert_eq!(captured.cid, before);

        let resumed = run_repair(&world.ctx(), "r2", Duration::from_secs(5), None)
            .await
            .unwrap();
        assert_eq!(resumed.state, "done", "{resumed:?}");
        assert_eq!(world.record_cid().await.unwrap(), before);
        let reader = world
            .actor_store
            .read(DID.to_owned(), world.blobstore.clone())
            .await
            .unwrap();
        assert_eq!(reader.repair_steps("r2").await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_client_change_between_steps_supersedes_the_repair() {
        let world = world().await;
        world.repairs.create("r3", DID, &republish()).await.unwrap();
        world.fence("r3");
        run_repair(
            &world.ctx(),
            "r3",
            Duration::from_secs(5),
            Some(StopAfterStep(1)),
        )
        .await
        .unwrap();
        // something else moves the root: the record comes back by another hand
        let mut txn = world
            .actor_store
            .transact_maintenance(DID.to_owned(), world.blobstore.clone(), "r3")
            .await
            .unwrap();
        txn.process_writes(vec![post("client wrote this")], None)
            .await
            .unwrap();
        drop(txn);
        publish_pending(&world.actor_store, &world.sequencer, DID, None)
            .await
            .unwrap();
        let clients = world.record_cid().await.unwrap();

        let resumed = run_repair(&world.ctx(), "r3", Duration::from_secs(5), None)
            .await
            .unwrap();
        assert_eq!(resumed.state, "client-superseded", "{resumed:?}");
        assert!(resumed.outcome.unwrap().contains("found a root other than"));
        assert_eq!(
            world.record_cid().await.unwrap(),
            clients,
            "never overwritten"
        );
    }

    #[tokio::test]
    async fn a_fence_moved_mid_repair_stops_the_next_step() {
        let world = world().await;
        world.repairs.create("r8", DID, &republish()).await.unwrap();
        world.fence("r8");
        run_repair(
            &world.ctx(),
            "r8",
            Duration::from_secs(5),
            Some(StopAfterStep(1)),
        )
        .await
        .unwrap();
        // the operator hands the actor to another workflow before step 2
        world.fence("someone-else");
        let err = run_repair(&world.ctx(), "r8", Duration::from_secs(5), None)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("not the maintenance workflow"),
            "{err}"
        );
        // a step that fails for any reason other than a moved root is an
        // error, not a superseded verdict
        world.fence("r8");
        let root = current_root(&world.ctx(), DID).await.unwrap();
        let mut foreign = post("elsewhere");
        if let PreparedWrite::Create(write) = &mut foreign {
            write.uri = "at://example.com/app.bsky.feed.post/3lfixtureaa2a".to_owned();
        }
        let failed = commit_step(&world.ctx(), "r8", DID, 2, &root, vec![foreign]).await;
        assert!(failed.is_err());
        assert!(commit_step(&world.ctx(), "r8", DID, 2, "not-a-cid", vec![])
            .await
            .is_err());
    }

    #[tokio::test]
    async fn a_missing_record_cannot_be_republished() {
        let world = world().await;
        world
            .repairs
            .create(
                "r4",
                DID,
                &RepairKind::Republish {
                    uri: "at://did:plc:repair/app.bsky.feed.post/3lfixturenope".to_owned(),
                    captured: None,
                },
            )
            .await
            .unwrap();
        world.fence("r4");
        let failed = run_repair(&world.ctx(), "r4", Duration::from_secs(5), None)
            .await
            .unwrap();
        assert_eq!(failed.state, "failed");
        assert!(failed.outcome.unwrap().contains("nothing to republish"));
    }

    #[tokio::test]
    async fn a_recovery_commit_rises_above_the_boundary_and_a_phantom_is_deleted() {
        let world = world().await;
        let boundary = "3zzzzzzzzzzzz".to_owned();
        world
            .repairs
            .create(
                "r5",
                DID,
                &RepairKind::EmptyCommit {
                    boundary: boundary.clone(),
                },
            )
            .await
            .unwrap();
        world.fence("r5");
        let events_before = world.event_types().await.len();
        let done = run_repair(&world.ctx(), "r5", Duration::from_secs(5), None)
            .await
            .unwrap();
        assert_eq!(done.state, "done", "{done:?}");
        assert_eq!(world.event_types().await.len(), events_before + 1);
        let reader = world
            .actor_store
            .read(DID.to_owned(), world.blobstore.clone())
            .await
            .unwrap();
        let root = reader
            .storage
            .read()
            .await
            .get_root_detailed()
            .await
            .unwrap();
        assert!(root.rev > boundary);
        assert_eq!(
            world.lifecycle.revision_floor(DID).await.unwrap(),
            Some(boundary)
        );
        assert_eq!(done.kind.name(), "empty-commit");

        world
            .repairs
            .create(
                "r6",
                DID,
                &RepairKind::PhantomDelete {
                    uri: "at://did:plc:repair/app.bsky.feed.post/3lfixturephan".to_owned(),
                    value: serde_json::json!({
                        "$type": "app.bsky.feed.post",
                        "text": "phantom",
                        "createdAt": "2023-01-01T00:00:00.000Z",
                    }),
                },
            )
            .await
            .unwrap();
        world.fence("r6");
        let done = run_repair(&world.ctx(), "r6", Duration::from_secs(5), None)
            .await
            .unwrap();
        assert_eq!(done.state, "done", "{done:?}");
        assert_eq!(world.event_types().await.len(), events_before + 3);
        assert_eq!(done.kind.name(), "phantom-delete");
        assert_eq!(
            serde_json::to_value(&done).unwrap()["kind"]["kind"],
            "phantom-delete"
        );
    }

    #[tokio::test]
    async fn a_repair_waits_for_client_writes_to_drain() {
        let world = world().await;
        world.repairs.create("r7", DID, &republish()).await.unwrap();
        // a write in flight when the fence lands must finish first
        let held = world
            .actor_store
            .transact(DID.to_owned(), world.blobstore.clone())
            .await
            .unwrap();
        world.fence("r7");
        let err = run_repair(&world.ctx(), "r7", Duration::from_millis(150), None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("in flight"), "{err}");
        drop(held);
        // another workflow holding the slot also keeps the repair out
        let slot = world.lock_dir.try_maintenance_slot(DID).unwrap().unwrap();
        let err = run_repair(&world.ctx(), "r7", Duration::from_millis(150), None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("maintenance"), "{err}");
        drop(slot);
        let done = run_repair(&world.ctx(), "r7", Duration::from_secs(5), None)
            .await
            .unwrap();
        assert_eq!(done.state, "done");
    }

    #[tokio::test]
    async fn a_quarantine_fences_the_event_and_closes_only_when_everything_is_settled() {
        let world = world().await;
        let rows = world
            .sequencer
            .sequencer
            .read()
            .await
            .rows_for_did_after(DID, 0)
            .await
            .unwrap();
        let seq = rows.last().unwrap().seq.unwrap();
        world.repairs.create("q1", DID, &republish()).await.unwrap();
        assert!(
            quarantine_seq(&world.ctx(), seq, DID, "bad-event", &["ghost".to_owned()])
                .await
                .is_err()
        );
        let opened = quarantine_seq(&world.ctx(), seq, DID, "bad-event", &["q1".to_owned()])
            .await
            .unwrap();
        assert_eq!(opened.state, "opened");
        assert_eq!(opened.steps_done, QUARANTINE_STEPS);
        assert!(opened.external_required);
        // the row no longer streams and the intent is superseded
        let live = world
            .sequencer
            .sequencer
            .read()
            .await
            .request_seq_range(crate::sequencer::RequestSeqRangeOpts {
                earliest_seq: None,
                latest_seq: None,
                earliest_time: None,
                limit: None,
            })
            .await
            .unwrap();
        assert!(live.iter().all(|evt| evt.seq() != seq));
        let reader = world
            .actor_store
            .read(DID.to_owned(), world.blobstore.clone())
            .await
            .unwrap();
        let intent = reader
            .all_intents()
            .await
            .unwrap()
            .into_iter()
            .find(|intent| intent.seq == Some(seq))
            .unwrap();
        assert_eq!(intent.state, "superseded");
        // rerunning is idempotent
        let again = quarantine_seq(&world.ctx(), seq, DID, "bad-event", &["q1".to_owned()])
            .await
            .unwrap();
        assert_eq!(again.steps_done.len(), 3);
        assert_eq!(
            world.repairs.open_quarantines_for(DID).await.unwrap(),
            [seq]
        );

        // closing needs the repair finished, the local index reconciled, and
        // an external outcome
        let err = close_quarantine(&world.ctx(), seq, ExternalOutcome::Verified, None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not finished"));
        world.fence("q1");
        let repaired = run_repair(&world.ctx(), "q1", Duration::from_secs(5), None)
            .await
            .unwrap();
        assert_eq!(repaired.state, "done", "{repaired:?}");
        let err = close_quarantine(&world.ctx(), seq, ExternalOutcome::Verified, None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not been reconciled"), "{err}");
        world.repairs.mark_local_reconciled(seq).await.unwrap();
        assert!(world
            .repairs
            .mark_local_reconciled(seq + 100)
            .await
            .is_err());
        let err = close_quarantine(&world.ctx(), seq, ExternalOutcome::Accepted, None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("justification"));
        let closed = close_quarantine(
            &world.ctx(),
            seq,
            ExternalOutcome::Accepted,
            Some("consumer re-fetched by hand"),
        )
        .await
        .unwrap();
        assert_eq!(closed.state, "closed");
        assert_eq!(closed.external_state, "accepted");
        assert!(closed.closed_at.is_some());
        assert!(world
            .repairs
            .open_quarantines_for(DID)
            .await
            .unwrap()
            .is_empty());
        // closing again is a no-op; an unknown seq is an error
        let again = close_quarantine(&world.ctx(), seq, ExternalOutcome::Verified, None)
            .await
            .unwrap();
        assert_eq!(again.external_state, "accepted");
        assert!(
            close_quarantine(&world.ctx(), seq + 100, ExternalOutcome::Verified, None)
                .await
                .is_err()
        );
        assert!(world.repairs.quarantine(seq + 100).await.unwrap().is_none());
        assert_eq!(
            serde_json::to_value(&closed).unwrap()["externalRequired"],
            true
        );

        // a quarantine with no linked repair closes as verified once the
        // local index was reconciled
        let earlier = rows.first().unwrap().seq.unwrap();
        quarantine_seq(&world.ctx(), earlier, DID, "bad-event", &[])
            .await
            .unwrap();
        world.repairs.mark_local_reconciled(earlier).await.unwrap();
        let verified = close_quarantine(&world.ctx(), earlier, ExternalOutcome::Verified, None)
            .await
            .unwrap();
        assert_eq!(verified.external_state, "verified");
        assert!(verified.justification.is_none());
    }

    #[test]
    fn repair_states_round_trip() {
        for state in [
            RepairState::Pending,
            RepairState::Running,
            RepairState::Done,
            RepairState::ClientSuperseded,
            RepairState::Failed,
        ] {
            assert_eq!(RepairState::parse(state.as_str()).unwrap(), state);
        }
        assert!(RepairState::parse("later").is_err());
        assert!(!RepairState::Running.is_terminal());
        assert!(RepairState::Failed.is_terminal());
        let err = anyhow::anyhow!("BadCommitSwapError: x");
        assert!(is_swap_failure(&err));
        assert!(!is_swap_failure(&anyhow::anyhow!("disk full")));
    }
}
