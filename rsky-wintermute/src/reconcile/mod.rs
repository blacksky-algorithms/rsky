//! Fenced reconciliation of one actor's downstream state against its
//! repository, and the admission rules every writer applies so that stale
//! work can never undo a reconciliation.
//!
//! Every writer takes a shared per-actor advisory lock before it checks the
//! fence; a reconcile takes the exclusive lock, so a writer cannot check
//! "no fence", pause, and commit after the fence is installed. Work is
//! admitted only above the actor's persisted boundary and only from the
//! actor's current acquisition generation.

pub mod frontier;
pub mod workflow;

use crate::types::WintermuteError;
use dashmap::DashMap;
use deadpool_postgres::{Client, Pool};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

/// Everything reconciliation persists lives in its own schema, beside the
/// appview's.
pub const SCHEMA_DDL: &str = "\
BEGIN;
SELECT pg_advisory_xact_lock(hashtext('wintermute.schema'));
CREATE SCHEMA IF NOT EXISTS wintermute;
CREATE TABLE IF NOT EXISTS wintermute.did_progress (
    did text PRIMARY KEY,
    max_rev text NOT NULL,
    last_commit_cid text,
    last_commit_rev text,
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE IF NOT EXISTS wintermute.did_generation (
    did text PRIMARY KEY,
    generation bigint NOT NULL
);
CREATE TABLE IF NOT EXISTS wintermute.reconcile_fence (
    did text PRIMARY KEY,
    workflow_id text NOT NULL,
    installed_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE IF NOT EXISTS wintermute.reconcile_boundary (
    did text PRIMARY KEY,
    rev text NOT NULL,
    frontier jsonb,
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE IF NOT EXISTS wintermute.reconcile_journal (
    did text PRIMARY KEY,
    workflow_id text NOT NULL,
    step text NOT NULL,
    branch text,
    boundary text,
    recovery_commit_cid text,
    obligation text,
    report jsonb,
    updated_at timestamptz NOT NULL DEFAULT now()
);
COMMIT;";

/// Where a job's data was obtained, captured before the data itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    /// A relay frame, by its sequence number.
    Firehose { seq: i64 },
    /// A repository export, by the host and the time the fetch started.
    Backfill { host: String, fetched_at: String },
    /// Records handed to the indexer directly by an operator tool.
    Direct,
}

/// The acquisition identity stamped on a job when its source is obtained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// The actor's generation when the source was obtained; `None` when
    /// the actor had never been reconciled.
    pub generation: Option<i64>,
    pub source: Source,
}

/// What a writer may do with a job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    Apply,
    /// The actor is fenced: keep the job and try again later.
    Deferred,
    /// The job's revision is at or below the persisted boundary.
    BelowBoundary,
    /// The job was obtained under an older generation; its source must be
    /// fetched again.
    StaleGeneration,
}

impl Admission {
    /// The metric label for the outcome.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Apply => "apply",
            Self::Deferred => "deferred",
            Self::BelowBoundary => "below_boundary",
            Self::StaleGeneration => "stale_generation",
        }
    }
}

/// How a writer identifies itself to the fence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateMode {
    /// An ordinary writer: fenced actors defer, boundaries and generations apply.
    Normal,
    /// The workflow holding the fence for the named reconciliation; nothing
    /// else is checked because the workflow writes the reconciled state.
    Workflow(String),
}

/// The per-actor state a writer read under its shared locks.
#[derive(Debug)]
pub struct Gate {
    mode: GateMode,
    /// Actors whose lock could not be taken because a reconcile holds it.
    busy: HashSet<String>,
    fences: HashMap<String, String>,
    generations: HashMap<String, i64>,
    boundaries: HashMap<String, String>,
}

impl Gate {
    /// Takes the shared lock of every actor in `dids` on `client` and reads
    /// their fences, generations, and boundaries. The locks are session
    /// level so that they outlive the autocommit statements of a batch;
    /// [`Gate::close`] releases them.
    pub async fn open(
        client: &Client,
        dids: &[&str],
        mode: GateMode,
    ) -> Result<Self, WintermuteError> {
        let mut sorted: Vec<&str> = dids
            .iter()
            .copied()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        sorted.sort_unstable();
        let keys: Vec<String> = sorted.iter().map(|did| (*did).to_owned()).collect();
        let rows = client
            .query(
                "SELECT d.did, pg_try_advisory_lock_shared(hashtext(d.did)) \
                 FROM unnest($1::text[]) AS d(did)",
                &[&keys],
            )
            .await?;
        let mut busy = HashSet::new();
        for row in rows {
            let did: String = row.get(0);
            let locked: bool = row.get(1);
            if !locked {
                busy.insert(did);
            }
        }
        let fences = client
            .query(
                "SELECT did, workflow_id FROM wintermute.reconcile_fence WHERE did = ANY($1)",
                &[&keys],
            )
            .await?
            .into_iter()
            .map(|row| (row.get::<_, String>(0), row.get::<_, String>(1)))
            .collect();
        let generations = client
            .query(
                "SELECT did, generation FROM wintermute.did_generation WHERE did = ANY($1)",
                &[&keys],
            )
            .await?
            .into_iter()
            .map(|row| (row.get::<_, String>(0), row.get::<_, i64>(1)))
            .collect();
        let boundaries = client
            .query(
                "SELECT did, rev FROM wintermute.reconcile_boundary WHERE did = ANY($1)",
                &[&keys],
            )
            .await?
            .into_iter()
            .map(|row| (row.get::<_, String>(0), row.get::<_, String>(1)))
            .collect();
        Ok(Self {
            mode,
            busy,
            fences,
            generations,
            boundaries,
        })
    }

    /// Whether a job for `did` at `rev` with `provenance` may be applied.
    #[must_use]
    pub fn admit(&self, did: &str, rev: &str, provenance: Option<&Provenance>) -> Admission {
        match &self.mode {
            GateMode::Workflow(workflow_id) => {
                return if self
                    .fences
                    .get(did)
                    .is_none_or(|holder| holder == workflow_id)
                {
                    Admission::Apply
                } else {
                    Admission::Deferred
                };
            }
            GateMode::Normal => {}
        }
        if self.busy.contains(did) || self.fences.contains_key(did) {
            return Admission::Deferred;
        }
        if let Some(boundary) = self.boundaries.get(did) {
            if rev <= boundary.as_str() {
                return Admission::BelowBoundary;
            }
        }
        if let Some(current) = self.generations.get(did) {
            let stamped = provenance.and_then(|provenance| provenance.generation);
            if stamped != Some(*current) {
                return Admission::StaleGeneration;
            }
        }
        Admission::Apply
    }

    /// Releases every advisory lock this connection holds.
    pub async fn close(self, client: &Client) -> Result<(), WintermuteError> {
        client
            .execute("SELECT pg_advisory_unlock_all()", &[])
            .await?;
        Ok(())
    }
}

/// Raises the actor's progress lower bound to `max_rev`.
pub async fn record_progress(
    client: &Client,
    dids_and_revs: &[(String, String)],
) -> Result<(), WintermuteError> {
    if dids_and_revs.is_empty() {
        return Ok(());
    }
    let dids: Vec<&str> = dids_and_revs.iter().map(|(did, _)| did.as_str()).collect();
    let revs: Vec<&str> = dids_and_revs.iter().map(|(_, rev)| rev.as_str()).collect();
    client
        .execute(
            "INSERT INTO wintermute.did_progress (did, max_rev) \
             SELECT * FROM unnest($1::text[], $2::text[]) \
             ON CONFLICT (did) DO UPDATE SET \
               max_rev = GREATEST(wintermute.did_progress.max_rev, EXCLUDED.max_rev), \
               updated_at = now()",
            &[&dids, &revs],
        )
        .await?;
    Ok(())
}

/// Records the commit an event delivered, the acknowledgement a recovery
/// waits for, and raises the progress bound to its revision.
pub async fn record_commit_progress(
    client: &Client,
    did: &str,
    rev: &str,
    commit_cid: &str,
) -> Result<(), WintermuteError> {
    client
        .execute(
            "INSERT INTO wintermute.did_progress (did, max_rev, last_commit_cid, last_commit_rev) \
             VALUES ($1, $2, $3, $2) \
             ON CONFLICT (did) DO UPDATE SET \
               max_rev = GREATEST(wintermute.did_progress.max_rev, EXCLUDED.max_rev), \
               last_commit_cid = EXCLUDED.last_commit_cid, \
               last_commit_rev = EXCLUDED.last_commit_rev, \
               updated_at = now()",
            &[&did, &rev, &commit_cid],
        )
        .await?;
    Ok(())
}

/// The progress row of an actor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Progress {
    pub max_rev: String,
    pub last_commit_cid: Option<String>,
    pub last_commit_rev: Option<String>,
}

pub async fn progress_of(client: &Client, did: &str) -> Result<Option<Progress>, WintermuteError> {
    let row = client
        .query_opt(
            "SELECT max_rev, last_commit_cid, last_commit_rev \
             FROM wintermute.did_progress WHERE did = $1",
            &[&did],
        )
        .await?;
    Ok(row.map(|row| Progress {
        max_rev: row.get(0),
        last_commit_cid: row.get(1),
        last_commit_rev: row.get(2),
    }))
}

pub async fn generation_of(client: &Client, did: &str) -> Result<Option<i64>, WintermuteError> {
    let row = client
        .query_opt(
            "SELECT generation FROM wintermute.did_generation WHERE did = $1",
            &[&did],
        )
        .await?;
    Ok(row.map(|row| row.get(0)))
}

const GENERATION_TTL: Duration = Duration::from_secs(2);

static GENERATION_CACHE: LazyLock<DashMap<String, (Option<i64>, Instant)>> =
    LazyLock::new(DashMap::new);

/// The generation to stamp on work obtained now for `did`, from a short
/// cache in front of the table. A stale answer only ever rejects more.
pub async fn current_generation(pool: &Pool, did: &str) -> Result<Option<i64>, WintermuteError> {
    if let Some(cached) = GENERATION_CACHE.get(did) {
        let (generation, at) = *cached;
        if at.elapsed() < GENERATION_TTL {
            return Ok(generation);
        }
    }
    let client = pool.get().await?;
    let generation = generation_of(&client, did).await?;
    GENERATION_CACHE.insert(did.to_owned(), (generation, Instant::now()));
    Ok(generation)
}

/// Forgets a cached generation, for a workflow that just changed it.
pub fn forget_generation(did: &str) {
    GENERATION_CACHE.remove(did);
}

#[cfg(test)]
mod tests;
