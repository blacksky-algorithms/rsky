//! The reconciliation workflow.
//!
//! Fence the actor, fetch a verified export and a fresh, complete
//! frontier, persist the boundary and a new generation, bring the
//! downstream state to the repository in both directions, then release the
//! application fence in the order the completion branch requires.

use super::frontier::{Frontier, FrontierClient};
use super::{GateMode, progress_of};
use crate::indexer::IndexerManager;
use crate::types::{IndexJob, WintermuteError, WriteAction};
use deadpool_postgres::{Client, Pool};
use lexicon_cid::Cid;
use rsky_repo::readable_repo::ReadableRepo;
use rsky_repo::storage::memory_blockstore::MemoryBlockstore;
use rsky_repo::storage::readable_blockstore::ReadableBlockstore;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

/// How many times a fetched export is retried to match the frontier's
/// current commit before the workflow gives up.
const ROOT_MATCH_ATTEMPTS: usize = 3;

#[derive(Debug, Clone)]
pub struct ReconcileOptions {
    pub did: String,
    pub workflow_id: String,
    /// Delete or overwrite downstream state newer than the repository.
    pub reset_to_repo: bool,
    /// Reconcile against an incomplete history, leaving an obligation that
    /// never converges.
    pub break_glass: bool,
    /// Report what would change without fencing or writing.
    pub dry_run: bool,
}

/// Which completion branch a reconciliation took.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Branch {
    /// The boundary equals the repository's revision: nothing to publish.
    Ordinary,
    /// The boundary exceeds the repository's revision: the PDS must publish
    /// a recovery commit above it, and the mutation fence stays until the
    /// acknowledgement is verified.
    Recovery,
}

/// Why a reconciliation stopped before changing anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Refusal {
    HistoryIncomplete {
        reason: Option<String>,
    },
    RootMismatch {
        frontier: Option<String>,
        export: String,
    },
    NewerDownstream {
        uris: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub did: String,
    pub workflow_id: String,
    pub dry_run: bool,
    pub repository_rev: String,
    pub repository_root: String,
    pub frontier: Frontier,
    pub progress_max_rev: Option<String>,
    pub boundary: String,
    pub generation: Option<i64>,
    pub branch: Option<Branch>,
    pub phantoms_deleted: usize,
    pub records_overwritten: usize,
    pub records_inserted: usize,
    pub refusal: Option<Refusal>,
    pub obligation: Option<String>,
    pub mutation_fence_note: String,
}

/// The records of a repository export.
#[derive(Debug)]
pub struct CarContents {
    pub root: Cid,
    pub rev: String,
    /// uri -> (cid, record json)
    pub records: HashMap<String, (String, serde_json::Value)>,
}

/// Reads and verifies an export: the root loads as a repository signed for
/// `did`, and every record is listed from the merkle search tree.
pub async fn car_contents(car_bytes: &[u8], did: &str) -> Result<CarContents, WintermuteError> {
    use rsky_repo::parse::get_and_parse_record;

    let mut reader = iroh_car::CarReader::new(Cursor::new(car_bytes.to_vec()))
        .await
        .map_err(|e| WintermuteError::Repo(format!("car read failed: {e}")))?;
    let root = *reader
        .header()
        .roots()
        .first()
        .ok_or_else(|| WintermuteError::Repo("no root cid".into()))?;
    let mut blocks = rsky_repo::block_map::BlockMap::new();
    while let Some((cid, data)) = reader
        .next_block()
        .await
        .map_err(|e| WintermuteError::Repo(format!("read block failed: {e}")))?
    {
        blocks.set(cid, data);
    }
    let blockstore = MemoryBlockstore::new(Some(blocks))
        .await
        .map_err(|e| WintermuteError::Repo(format!("blockstore failed: {e}")))?;
    let storage = Arc::new(tokio::sync::RwLock::new(blockstore));
    let mut repo = ReadableRepo::load(storage.clone(), root)
        .await
        .map_err(|e| WintermuteError::Repo(format!("repo load failed: {e}")))?;
    if repo.did() != did {
        return Err(WintermuteError::Repo(format!(
            "did mismatch: expected {did}, got {}",
            repo.did()
        )));
    }
    let leaves = repo
        .data
        .list(None, None, None)
        .await
        .map_err(|e| WintermuteError::Repo(format!("list failed: {e}")))?;
    let blocks = {
        let guard = storage.read().await;
        guard
            .get_blocks(leaves.iter().map(|entry| entry.value).collect())
            .await
            .map_err(|e| WintermuteError::Repo(format!("get blocks failed: {e}")))?
    };
    let mut records = HashMap::with_capacity(leaves.len());
    for entry in &leaves {
        let parsed = get_and_parse_record(&blocks.blocks, entry.value)
            .map_err(|e| WintermuteError::Repo(format!("record {} unreadable: {e}", entry.key)))?;
        let json = serde_json::to_value(&parsed.record)
            .map_err(|e| WintermuteError::Serialization(format!("json failed: {e}")))?;
        let json = crate::backfiller::convert_record_to_ipld(&json);
        records.insert(
            format!("at://{did}/{}", entry.key),
            (entry.value.to_string(), json),
        );
    }
    Ok(CarContents {
        root,
        rev: repo.commit.rev.clone(),
        records,
    })
}

struct Downstream {
    cid: String,
    rev: String,
}

async fn downstream_records(
    client: &Client,
    did: &str,
) -> Result<HashMap<String, Downstream>, WintermuteError> {
    let rows = client
        .query(
            "SELECT uri, cid, COALESCE(rev, '') FROM record WHERE did = $1",
            &[&did],
        )
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            (
                row.get::<_, String>(0),
                Downstream {
                    cid: row.get(1),
                    rev: row.get(2),
                },
            )
        })
        .collect())
}

/// The highest of the revisions present.
fn greatest<'a>(revs: impl IntoIterator<Item = Option<&'a str>>) -> Option<String> {
    revs.into_iter()
        .flatten()
        .max()
        .map(std::borrow::ToOwned::to_owned)
}

/// Fences the actor for one workflow. A fence another workflow holds is
/// never taken over: the actor stays as that workflow left it until it
/// finishes or an operator resumes it under the same id.
async fn install_fence(
    client: &mut Client,
    did: &str,
    workflow_id: &str,
) -> Result<(), WintermuteError> {
    let txn = client.transaction().await?;
    txn.execute("SELECT pg_advisory_xact_lock(hashtext($1))", &[&did])
        .await?;
    let held: Option<String> = txn
        .query_opt(
            "SELECT workflow_id FROM wintermute.reconcile_fence WHERE did = $1",
            &[&did],
        )
        .await?
        .map(|row| row.get(0));
    if let Some(held) = held.filter(|held| held != workflow_id) {
        return Err(WintermuteError::Other(format!(
            "{did} is fenced by workflow {held}; it stays fenced"
        )));
    }
    txn.execute(
        "INSERT INTO wintermute.reconcile_fence (did, workflow_id) VALUES ($1, $2) \
         ON CONFLICT (did) DO UPDATE SET workflow_id = EXCLUDED.workflow_id, installed_at = now()",
        &[&did, &workflow_id],
    )
    .await?;
    txn.execute(
        "INSERT INTO wintermute.reconcile_journal (did, workflow_id, step) VALUES ($1, $2, 'fenced') \
         ON CONFLICT (did) DO UPDATE SET workflow_id = EXCLUDED.workflow_id, step = 'fenced', \
           branch = NULL, boundary = NULL, recovery_commit_cid = NULL, obligation = NULL, \
           report = NULL, updated_at = now()",
        &[&did, &workflow_id],
    )
    .await?;
    txn.commit().await?;
    Ok(())
}

/// Removes the application fence the named workflow holds so queued events
/// for the actor apply; a fence held by another workflow is left alone.
pub async fn release_fence(
    client: &Client,
    did: &str,
    workflow_id: &str,
) -> Result<bool, WintermuteError> {
    let removed = client
        .execute(
            "DELETE FROM wintermute.reconcile_fence WHERE did = $1 AND workflow_id = $2",
            &[&did, &workflow_id],
        )
        .await?;
    Ok(removed > 0)
}

/// The journal's step and workflow for an actor, if a workflow ever ran.
pub async fn journal_of(
    client: &Client,
    did: &str,
) -> Result<Option<(String, String)>, WintermuteError> {
    let row = client
        .query_opt(
            "SELECT step, workflow_id FROM wintermute.reconcile_journal WHERE did = $1",
            &[&did],
        )
        .await?;
    Ok(row.map(|row| (row.get(0), row.get(1))))
}

async fn journal_step(
    client: &Client,
    did: &str,
    step: &str,
    branch: Option<Branch>,
    boundary: Option<&str>,
    obligation: Option<&str>,
    report: Option<&serde_json::Value>,
) -> Result<(), WintermuteError> {
    let branch = branch.map(|branch| match branch {
        Branch::Ordinary => "ordinary",
        Branch::Recovery => "recovery",
    });
    client
        .execute(
            "UPDATE wintermute.reconcile_journal SET step = $2, branch = COALESCE($3, branch), \
               boundary = COALESCE($4, boundary), obligation = COALESCE($5, obligation), \
               report = COALESCE($6, report), updated_at = now() WHERE did = $1",
            &[&did, &step, &branch, &boundary, &obligation, &report],
        )
        .await?;
    Ok(())
}

async fn persist_boundary(
    client: &mut Client,
    did: &str,
    boundary: &str,
    frontier: &Frontier,
) -> Result<i64, WintermuteError> {
    let txn = client.transaction().await?;
    txn.execute("SELECT pg_advisory_xact_lock(hashtext($1))", &[&did])
        .await?;
    let evidence = serde_json::to_value(frontier)
        .map_err(|e| WintermuteError::Serialization(format!("frontier json: {e}")))?;
    txn.execute(
        "INSERT INTO wintermute.reconcile_boundary (did, rev, frontier) VALUES ($1, $2, $3) \
         ON CONFLICT (did) DO UPDATE SET rev = GREATEST(wintermute.reconcile_boundary.rev, EXCLUDED.rev), \
           frontier = EXCLUDED.frontier, updated_at = now()",
        &[&did, &boundary, &evidence],
    )
    .await?;
    let generation: i64 = txn
        .query_one(
            "INSERT INTO wintermute.did_generation (did, generation) VALUES ($1, 1) \
             ON CONFLICT (did) DO UPDATE SET generation = wintermute.did_generation.generation + 1 \
             RETURNING generation",
            &[&did],
        )
        .await?
        .get(0);
    txn.commit().await?;
    Ok(generation)
}

/// The boundary persisted for an actor, if any.
pub async fn persisted_boundary(
    client: &Client,
    did: &str,
) -> Result<Option<String>, WintermuteError> {
    let row = client
        .query_opt(
            "SELECT rev FROM wintermute.reconcile_boundary WHERE did = $1",
            &[&did],
        )
        .await?;
    Ok(row.map(|row| row.get(0)))
}

fn now() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

fn delete_job(uri: &str, rev: &str) -> (Vec<u8>, IndexJob) {
    (
        uri.as_bytes().to_vec(),
        IndexJob {
            uri: uri.to_owned(),
            cid: String::new(),
            action: WriteAction::Delete,
            record: None,
            indexed_at: now(),
            rev: rev.to_owned(),
            provenance: None,
        },
    )
}

fn create_job(uri: &str, cid: &str, json: &serde_json::Value, rev: &str) -> (Vec<u8>, IndexJob) {
    (
        format!("{uri}#create").into_bytes(),
        IndexJob {
            uri: uri.to_owned(),
            cid: cid.to_owned(),
            action: WriteAction::Create,
            record: Some(json.clone()),
            indexed_at: now(),
            rev: rev.to_owned(),
            provenance: None,
        },
    )
}

/// The plan for one reconciliation, before anything is written.
struct Plan {
    jobs: Vec<(Vec<u8>, IndexJob)>,
    phantoms_deleted: usize,
    records_overwritten: usize,
    records_inserted: usize,
    /// Downstream state newer than the repository that only
    /// `reset_to_repo` may override.
    newer_downstream: Vec<String>,
}

fn plan(
    contents: &CarContents,
    downstream: &HashMap<String, Downstream>,
    frontier_rev: Option<&str>,
    reset_to_repo: bool,
) -> Plan {
    let repo_rev = contents.rev.as_str();
    let mut plan = Plan {
        jobs: Vec::new(),
        phantoms_deleted: 0,
        records_overwritten: 0,
        records_inserted: 0,
        newer_downstream: Vec::new(),
    };
    let mut uris: Vec<&String> = downstream.keys().collect();
    uris.sort();
    for uri in uris {
        let stored = &downstream[uri];
        match contents.records.get(uri) {
            None => {
                if stored.rev.as_str() <= repo_rev {
                    plan.jobs.push(delete_job(uri, repo_rev));
                    plan.phantoms_deleted += 1;
                } else if reset_to_repo {
                    plan.jobs.push(delete_job(uri, &stored.rev));
                    plan.phantoms_deleted += 1;
                } else {
                    plan.newer_downstream.push(uri.clone());
                }
            }
            Some((cid, json)) if *cid != stored.cid => {
                if stored.rev.as_str() <= repo_rev {
                    plan.jobs.push(create_job(uri, cid, json, repo_rev));
                    plan.records_overwritten += 1;
                } else if reset_to_repo {
                    plan.jobs.push(delete_job(uri, &stored.rev));
                    plan.jobs.push(create_job(uri, cid, json, repo_rev));
                    plan.records_overwritten += 1;
                } else {
                    plan.newer_downstream.push(uri.clone());
                }
            }
            Some(_) => {}
        }
    }
    // absent downstream: an insert is safe only when no newer downstream
    // history could explain the absence
    let newer_history = frontier_rev.is_some_and(|frontier| frontier > repo_rev);
    let mut missing: Vec<(&String, &(String, serde_json::Value))> = contents
        .records
        .iter()
        .filter(|(uri, _)| !downstream.contains_key(*uri))
        .collect();
    missing.sort_by(|a, b| a.0.cmp(b.0));
    for (uri, (cid, json)) in missing {
        if !newer_history || reset_to_repo {
            plan.jobs.push(create_job(uri, cid, json, repo_rev));
            plan.records_inserted += 1;
        } else {
            plan.newer_downstream.push(uri.clone());
        }
    }
    plan
}

/// Runs the workflow for one actor.
///
/// A refusal before anything durable happened leaves the actor as it was,
/// with the fence released; a failure after the boundary was persisted
/// keeps the fence, and the recovery branch keeps it until
/// `verify_recovery` lets the recovery commit through. A dry run installs
/// and releases nothing.
pub async fn reconcile(
    pool: &Pool,
    pds: &FrontierClient,
    opts: &ReconcileOptions,
) -> Result<Report, WintermuteError> {
    let mut client = pool.get().await?;
    let did = opts.did.as_str();
    if !opts.dry_run {
        install_fence(&mut client, did, &opts.workflow_id).await?;
    }
    let outcome = reconcile_fenced(pool, &mut client, pds, opts).await;
    if opts.dry_run {
        return outcome;
    }
    let release = match &outcome {
        Ok(report) => report.branch != Some(Branch::Recovery),
        Err(_) => matches!(
            journal_of(&client, did).await?,
            Some((step, _)) if step == "fenced"
        ),
    };
    if release {
        release_fence(&client, did, &opts.workflow_id).await?;
    }
    outcome
}

async fn reconcile_fenced(
    pool: &Pool,
    client: &mut Client,
    pds: &FrontierClient,
    opts: &ReconcileOptions,
) -> Result<Report, WintermuteError> {
    let did = opts.did.as_str();
    let mut contents = car_contents(&pds.repo_car(did).await?, did).await?;
    let mut frontier = pds.frontier(did).await?;
    let mut attempts = 1;
    while frontier.current_commit_cid.as_deref() != Some(contents.root.to_string().as_str()) {
        if attempts >= ROOT_MATCH_ATTEMPTS {
            return refuse(
                client,
                opts,
                &contents,
                &frontier,
                Refusal::RootMismatch {
                    frontier: frontier.current_commit_cid.clone(),
                    export: contents.root.to_string(),
                },
            )
            .await;
        }
        attempts += 1;
        contents = car_contents(&pds.repo_car(did).await?, did).await?;
        frontier = pds.frontier(did).await?;
    }
    let progress = progress_of(client, did).await?;
    let progress_rev = progress.as_ref().map(|progress| progress.max_rev.clone());
    if !frontier.complete && !opts.break_glass {
        return refuse(
            client,
            opts,
            &contents,
            &frontier,
            Refusal::HistoryIncomplete {
                reason: frontier.reason.clone(),
            },
        )
        .await;
    }
    let history_complete = frontier.complete;
    // F: the complete frontier when the PDS proves one, else the local
    // lower bound alone under break-glass
    let frontier_rev = if history_complete {
        greatest([
            progress_rev.as_deref(),
            frontier.publication_max_rev.as_deref(),
        ])
    } else {
        progress_rev.clone()
    };
    let existing_boundary = persisted_boundary(client, did).await?;
    let boundary = greatest([
        existing_boundary.as_deref(),
        frontier_rev.as_deref(),
        Some(contents.rev.as_str()),
        frontier.exposed_max_rev.as_deref(),
    ])
    .unwrap_or_else(|| contents.rev.clone());
    let downstream = downstream_records(client, did).await?;
    let plan = plan(
        &contents,
        &downstream,
        frontier_rev.as_deref(),
        opts.reset_to_repo,
    );
    if !plan.newer_downstream.is_empty() {
        return refuse(
            client,
            opts,
            &contents,
            &frontier,
            Refusal::NewerDownstream {
                uris: plan.newer_downstream,
            },
        )
        .await;
    }
    let branch = if boundary.as_str() > contents.rev.as_str() {
        Branch::Recovery
    } else {
        Branch::Ordinary
    };
    let mut report = Report {
        did: did.to_owned(),
        workflow_id: opts.workflow_id.clone(),
        dry_run: opts.dry_run,
        repository_rev: contents.rev.clone(),
        repository_root: contents.root.to_string(),
        frontier: frontier.clone(),
        progress_max_rev: progress_rev,
        boundary: boundary.clone(),
        generation: None,
        branch: Some(branch),
        phantoms_deleted: plan.phantoms_deleted,
        records_overwritten: plan.records_overwritten,
        records_inserted: plan.records_inserted,
        refusal: None,
        obligation: (!history_complete).then(|| "history-incomplete-accepted".to_owned()),
        mutation_fence_note: match branch {
            Branch::Ordinary => "release the PDS mutation fence".to_owned(),
            Branch::Recovery => format!(
                "publish a recovery commit above {boundary} on the PDS, then verify its \
                 acknowledgement before releasing the PDS mutation fence"
            ),
        },
    };
    if opts.dry_run {
        return Ok(report);
    }
    // the boundary and generation are persisted before any destructive
    // change, so a crash leaves the actor fenced with its history known
    if history_complete {
        report.generation = Some(persist_boundary(&mut *client, did, &boundary, &frontier).await?);
        journal_step(
            client,
            did,
            "boundary",
            Some(branch),
            Some(&boundary),
            None,
            None,
        )
        .await?;
    } else {
        journal_step(
            client,
            did,
            "boundary",
            Some(branch),
            None,
            Some("history-incomplete-accepted"),
            None,
        )
        .await?;
    }
    if !plan.jobs.is_empty() {
        let (results, failed) = IndexerManager::process_jobs_batch_with(
            pool,
            &plan.jobs,
            false,
            false,
            GateMode::Workflow(opts.workflow_id.clone()),
        )
        .await;
        if failed {
            return Err(WintermuteError::Other(
                "reconciliation writes failed; the actor stays fenced".into(),
            ));
        }
        if let Some((_, Err(e))) = results.iter().find(|(_, result)| result.is_err()) {
            return Err(WintermuteError::Other(format!(
                "reconciliation write failed: {e}; the actor stays fenced"
            )));
        }
    }
    if history_complete
        && persisted_boundary(client, did).await?.as_deref() != Some(boundary.as_str())
    {
        return Err(WintermuteError::Other(
            "the persisted boundary changed during reconciliation".into(),
        ));
    }
    let evidence = serde_json::to_value(&report)
        .map_err(|e| WintermuteError::Serialization(format!("report json: {e}")))?;
    let step = match branch {
        Branch::Ordinary => "done",
        Branch::Recovery => "awaiting-recovery",
    };
    journal_step(client, did, step, Some(branch), None, None, Some(&evidence)).await?;
    Ok(report)
}

async fn refuse(
    client: &Client,
    opts: &ReconcileOptions,
    contents: &CarContents,
    frontier: &Frontier,
    refusal: Refusal,
) -> Result<Report, WintermuteError> {
    let progress = progress_of(client, &opts.did).await?;
    let report = Report {
        did: opts.did.clone(),
        workflow_id: opts.workflow_id.clone(),
        dry_run: opts.dry_run,
        repository_rev: contents.rev.clone(),
        repository_root: contents.root.to_string(),
        frontier: frontier.clone(),
        progress_max_rev: progress.map(|progress| progress.max_rev),
        boundary: persisted_boundary(client, &opts.did)
            .await?
            .unwrap_or_default(),
        generation: None,
        branch: None,
        phantoms_deleted: 0,
        records_overwritten: 0,
        records_inserted: 0,
        refusal: Some(refusal),
        obligation: None,
        mutation_fence_note: "nothing was changed".to_owned(),
    };
    if !opts.dry_run {
        let evidence = serde_json::to_value(&report)
            .map_err(|e| WintermuteError::Serialization(format!("report json: {e}")))?;
        journal_step(
            client,
            &opts.did,
            "refused",
            None,
            None,
            None,
            Some(&evidence),
        )
        .await?;
    }
    Ok(report)
}

/// Whether the recovery commit has been applied downstream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Verification {
    pub did: String,
    pub expected_commit_cid: String,
    pub last_commit_cid: Option<String>,
    pub last_commit_rev: Option<String>,
    pub acknowledged: bool,
}

/// Checks the acknowledgement oracle: the durable commit-level progress
/// names the recovery commit. Record counts are never accepted instead.
pub async fn verify_recovery(
    pool: &Pool,
    did: &str,
    expected_commit_cid: &str,
) -> Result<Verification, WintermuteError> {
    let client = pool.get().await?;
    // the recovery commit was published while the application fence held;
    // lifting the workflow's own fence lets the queued commit apply
    if let Some((step, workflow_id)) = journal_of(&client, did).await? {
        if step == "awaiting-recovery" && release_fence(&client, did, &workflow_id).await? {
            tracing::info!(%did, %workflow_id, "application fence released for the recovery commit");
        }
    }
    let progress = progress_of(&client, did).await?;
    let last_commit_cid = progress
        .as_ref()
        .and_then(|progress| progress.last_commit_cid.clone());
    let acknowledged = last_commit_cid.as_deref() == Some(expected_commit_cid);
    if acknowledged {
        client
            .execute(
                "UPDATE wintermute.reconcile_journal SET step = 'done', recovery_commit_cid = $2, \
                   updated_at = now() WHERE did = $1 AND step = 'awaiting-recovery'",
                &[&did, &expected_commit_cid],
            )
            .await?;
    }
    Ok(Verification {
        did: did.to_owned(),
        expected_commit_cid: expected_commit_cid.to_owned(),
        last_commit_cid,
        last_commit_rev: progress.and_then(|progress| progress.last_commit_rev),
        acknowledged,
    })
}
