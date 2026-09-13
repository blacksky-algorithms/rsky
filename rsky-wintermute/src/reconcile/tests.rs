//! Reconciliation against a live Postgres (`DATABASE_URL`, default
//! `postgresql://postgres:postgres@localhost:5432/bsky_test`) with the PDS
//! mocked. The tables the workflow touches are created here, so a stock
//! database suffices.

use super::frontier::{Frontier, FrontierClient};
use super::workflow::{
    Branch, ReconcileOptions, Refusal, car_contents, reconcile, verify_recovery,
};
use super::{Admission, Gate, GateMode, Provenance, Source, progress_of};
use crate::indexer::IndexerManager;
use crate::types::{IndexJob, WriteAction};
use deadpool_postgres::Pool;
use rsky_repo::block_map::BlockMap;
use rsky_repo::repo::Repo;
use rsky_repo::storage::memory_blockstore::MemoryBlockstore;
use rsky_repo::types::{
    RecordCreateOrUpdateOp, RecordDeleteOp, RecordWriteEnum, RecordWriteOp, RepoRecord,
    WriteOpAction,
};
use secp256k1::Keypair;
use serde_json::json;
use std::sync::Arc;

const TABLES_DDL: &str = "\
CREATE TABLE IF NOT EXISTS actor (
    did varchar PRIMARY KEY,
    handle varchar UNIQUE,
    \"indexedAt\" varchar NOT NULL,
    \"takedownRef\" varchar,
    \"upstreamStatus\" varchar
);
CREATE TABLE IF NOT EXISTS record (
    uri varchar PRIMARY KEY,
    cid varchar NOT NULL,
    did varchar NOT NULL,
    json text NOT NULL,
    \"indexedAt\" varchar NOT NULL,
    rev varchar,
    \"takedownRef\" varchar
);
CREATE TABLE IF NOT EXISTS duplicate_record (
    uri varchar PRIMARY KEY,
    cid varchar NOT NULL,
    \"duplicateOf\" varchar NOT NULL,
    \"indexedAt\" varchar NOT NULL
);";

async fn pool() -> Pool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5432/bsky_test".to_owned());
    let pool = crate::config::create_pg_pool(&url, crate::config::pg_pool_config(8)).unwrap();
    let client = pool.get().await.unwrap();
    client.batch_execute(TABLES_DDL).await.unwrap();
    pool
}

async fn reset_actor(pool: &Pool, did: &str) {
    let client = pool.get().await.unwrap();
    for statement in [
        "DELETE FROM record WHERE did = $1",
        "DELETE FROM actor WHERE did = $1",
        "DELETE FROM wintermute.did_progress WHERE did = $1",
        "DELETE FROM wintermute.did_generation WHERE did = $1",
        "DELETE FROM wintermute.reconcile_fence WHERE did = $1",
        "DELETE FROM wintermute.reconcile_boundary WHERE did = $1",
        "DELETE FROM wintermute.reconcile_journal WHERE did = $1",
    ] {
        client.execute(statement, &[&did]).await.unwrap();
    }
    super::forget_generation(did);
}

fn keypair() -> Keypair {
    let seed: [u8; 32] = rand::random();
    Keypair::from_seckey_slice(secp256k1::SECP256K1, &seed).unwrap()
}

fn note(rkey: &str, text: &str) -> RecordCreateOrUpdateOp {
    let record: RepoRecord = serde_json::from_value(json!({
        "$type": "com.example.note",
        "text": text,
        "createdAt": "2026-01-01T00:00:00.000Z",
    }))
    .unwrap();
    RecordCreateOrUpdateOp {
        action: WriteOpAction::Create,
        collection: "com.example.note".to_owned(),
        rkey: rkey.to_owned(),
        record,
    }
}

/// A repository built through the real commit path, exported as a CAR.
struct TestRepo {
    did: String,
    keypair: Keypair,
    repo: Repo,
    storage: Arc<tokio::sync::RwLock<MemoryBlockstore>>,
}

impl TestRepo {
    async fn create(did: &str, notes: &[(&str, &str)]) -> Self {
        let keypair = keypair();
        let storage = Arc::new(tokio::sync::RwLock::new(
            MemoryBlockstore::new(None).await.unwrap(),
        ));
        let writes: Vec<RecordCreateOrUpdateOp> =
            notes.iter().map(|(rkey, text)| note(rkey, text)).collect();
        let repo = Repo::create(storage.clone(), did.to_owned(), &keypair, Some(writes))
            .await
            .unwrap();
        Self {
            did: did.to_owned(),
            keypair,
            repo,
            storage,
        }
    }

    async fn write(&mut self, ops: Vec<RecordWriteOp>) {
        let mut repo = Repo::load(self.storage.clone(), Some(self.repo.cid))
            .await
            .unwrap();
        self.repo = repo
            .apply_writes(RecordWriteEnum::List(ops), &self.keypair)
            .await
            .unwrap();
    }

    fn rev(&self) -> String {
        self.repo.commit.rev.clone()
    }

    fn root(&self) -> String {
        self.repo.cid.to_string()
    }

    async fn car(&self) -> Vec<u8> {
        let blocks: BlockMap = self.storage.read().await.blocks.read().await.clone();
        rsky_repo::car::blocks_to_car_file(Some(&self.repo.cid), blocks)
            .await
            .unwrap()
    }

    fn uri(&self, rkey: &str) -> String {
        format!("at://{}/com.example.note/{rkey}", self.did)
    }
}

fn frontier(did: &str, root: &str, publication_max_rev: &str, complete: bool) -> Frontier {
    Frontier {
        did: did.to_owned(),
        publication_max_rev: Some(publication_max_rev.to_owned()),
        exposed_max_rev: None,
        current_commit_cid: Some(root.to_owned()),
        signed_commit_rev: Some(publication_max_rev.to_owned()),
        repo_root_rev: Some(publication_max_rev.to_owned()),
        restore_event_count: 0,
        lifetime: "hostedHereOnly".to_owned(),
        genesis_kind: "creation".to_owned(),
        complete,
        reason: (!complete).then(|| "test".to_owned()),
    }
}

/// A PDS answering the frontier and the export for one actor.
struct MockPds {
    _server: mockito::ServerGuard,
    client: FrontierClient,
}

impl MockPds {
    async fn serving(did: &str, car: Vec<u8>, frontier: &Frontier) -> Self {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/xrpc/community.blacksky.pds.getPublicationFrontier")
            .match_query(mockito::Matcher::UrlEncoded("did".into(), did.into()))
            .match_header("authorization", mockito::Matcher::Regex("^Basic ".into()))
            .with_status(200)
            .with_body(serde_json::to_vec(frontier).unwrap())
            .expect_at_least(1)
            .create_async()
            .await;
        server
            .mock("GET", "/xrpc/com.atproto.sync.getRepo")
            .match_query(mockito::Matcher::UrlEncoded("did".into(), did.into()))
            .with_status(200)
            .with_body(car)
            .expect_at_least(1)
            .create_async()
            .await;
        let client = FrontierClient::new(&server.url(), "secret").unwrap();
        Self {
            _server: server,
            client,
        }
    }
}

fn job(uri: &str, cid: &str, rev: &str, action: WriteAction, generation: Option<i64>) -> IndexJob {
    IndexJob {
        uri: uri.to_owned(),
        cid: cid.to_owned(),
        action,
        record: Some(json!({"$type": "com.example.note", "text": rev})),
        indexed_at: "2026-01-01T00:00:00.000Z".to_owned(),
        rev: rev.to_owned(),
        provenance: Some(Provenance {
            generation,
            source: Source::Firehose { seq: 1 },
        }),
    }
}

async fn stored_revs(pool: &Pool, did: &str) -> Vec<(String, String, String)> {
    let client = pool.get().await.unwrap();
    let mut rows: Vec<(String, String, String)> = client
        .query(
            "SELECT uri, cid, COALESCE(rev, '') FROM record WHERE did = $1",
            &[&did],
        )
        .await
        .unwrap()
        .into_iter()
        .map(|row| (row.get(0), row.get(1), row.get(2)))
        .collect();
    rows.sort();
    rows
}

fn options(did: &str) -> ReconcileOptions {
    ReconcileOptions {
        did: did.to_owned(),
        workflow_id: "wf-test".to_owned(),
        reset_to_repo: false,
        break_glass: false,
        dry_run: false,
    }
}

#[tokio::test]
async fn the_gate_admits_defers_and_refuses_by_rule() {
    let pool = pool().await;
    let did = "did:plc:wintermute-gate";
    reset_actor(&pool, did).await;
    let client = pool.get().await.unwrap();
    let stamped = Provenance {
        generation: Some(1),
        source: Source::Direct,
    };

    // nothing known about the actor: everything applies
    let gate = Gate::open(&client, &[did], GateMode::Normal).await.unwrap();
    assert_eq!(gate.admit(did, "3a", None), Admission::Apply);
    gate.close(&client).await.unwrap();

    // a fence defers ordinary writers and admits its own workflow
    client
        .execute(
            "INSERT INTO wintermute.reconcile_fence (did, workflow_id) VALUES ($1, 'wf-1')",
            &[&did],
        )
        .await
        .unwrap();
    let gate = Gate::open(&client, &[did], GateMode::Normal).await.unwrap();
    assert_eq!(gate.admit(did, "3a", None), Admission::Deferred);
    gate.close(&client).await.unwrap();
    let gate = Gate::open(&client, &[did], GateMode::Workflow("wf-1".into()))
        .await
        .unwrap();
    assert_eq!(gate.admit(did, "3a", None), Admission::Apply);
    gate.close(&client).await.unwrap();
    let gate = Gate::open(&client, &[did], GateMode::Workflow("wf-2".into()))
        .await
        .unwrap();
    assert_eq!(gate.admit(did, "3a", None), Admission::Deferred);
    gate.close(&client).await.unwrap();
    client
        .execute(
            "DELETE FROM wintermute.reconcile_fence WHERE did = $1",
            &[&did],
        )
        .await
        .unwrap();

    // the boundary refuses work at or below it, for any action
    client
        .execute(
            "INSERT INTO wintermute.reconcile_boundary (did, rev) VALUES ($1, '3b')",
            &[&did],
        )
        .await
        .unwrap();
    client
        .execute(
            "INSERT INTO wintermute.did_generation (did, generation) VALUES ($1, 1)",
            &[&did],
        )
        .await
        .unwrap();
    let gate = Gate::open(&client, &[did], GateMode::Normal).await.unwrap();
    assert_eq!(
        gate.admit(did, "3a", Some(&stamped)),
        Admission::BelowBoundary
    );
    assert_eq!(
        gate.admit(did, "3b", Some(&stamped)),
        Admission::BelowBoundary
    );
    assert_eq!(gate.admit(did, "3c", Some(&stamped)), Admission::Apply);
    // an older or missing generation is refused above the boundary too
    assert_eq!(gate.admit(did, "3c", None), Admission::StaleGeneration);
    let older = Provenance {
        generation: Some(0),
        source: Source::Direct,
    };
    assert_eq!(
        gate.admit(did, "3c", Some(&older)),
        Admission::StaleGeneration
    );
    assert_eq!(gate.admit("did:plc:other", "3a", None), Admission::Apply);
    assert_eq!(Admission::Deferred.label(), "deferred");
    gate.close(&client).await.unwrap();

    // an exclusive holder makes the shared lock unavailable: deferred
    let mut holder = pool.get().await.unwrap();
    let txn = holder.transaction().await.unwrap();
    txn.execute("SELECT pg_advisory_xact_lock(hashtext($1))", &[&did])
        .await
        .unwrap();
    let gate = Gate::open(&client, &[did], GateMode::Normal).await.unwrap();
    assert_eq!(gate.admit(did, "3c", Some(&stamped)), Admission::Deferred);
    gate.close(&client).await.unwrap();
    txn.rollback().await.unwrap();

    // progress is a lower bound raised with GREATEST
    super::record_progress(&client, &[(did.to_owned(), "3c".to_owned())])
        .await
        .unwrap();
    super::record_progress(&client, &[(did.to_owned(), "3a".to_owned())])
        .await
        .unwrap();
    super::record_progress(&client, &[]).await.unwrap();
    assert_eq!(
        progress_of(&client, did).await.unwrap().unwrap().max_rev,
        "3c"
    );
    super::record_commit_progress(&client, did, "3d", "bafycommit")
        .await
        .unwrap();
    let progress = progress_of(&client, did).await.unwrap().unwrap();
    assert_eq!(progress.max_rev, "3d");
    assert_eq!(progress.last_commit_cid.as_deref(), Some("bafycommit"));
    assert_eq!(super::generation_of(&client, did).await.unwrap(), Some(1));
    assert_eq!(
        super::current_generation(&pool, did).await.unwrap(),
        Some(1)
    );
    // the cache answers until it is forgotten
    client
        .execute(
            "UPDATE wintermute.did_generation SET generation = 2 WHERE did = $1",
            &[&did],
        )
        .await
        .unwrap();
    assert_eq!(
        super::current_generation(&pool, did).await.unwrap(),
        Some(1)
    );
    super::forget_generation(did);
    assert_eq!(
        super::current_generation(&pool, did).await.unwrap(),
        Some(2)
    );
    reset_actor(&pool, did).await;
}

#[tokio::test]
async fn ordinary_reconcile_removes_phantoms_and_restores_missing_records() {
    let pool = pool().await;
    let did = "did:plc:wintermute-ordinary";
    reset_actor(&pool, did).await;
    let repo = TestRepo::create(did, &[("aaa", "keep"), ("bbb", "restore")]).await;
    let rev = repo.rev();
    // downstream: a phantom at an older revision, a stale value for aaa,
    // and bbb missing
    let old_rev = format!("{}0", &rev[..rev.len() - 1]);
    let phantom = repo.uri("zzz");
    let stale = job(
        &repo.uri("aaa"),
        "bafystale",
        &old_rev,
        WriteAction::Create,
        None,
    );
    IndexerManager::process_job(&pool, &stale, false)
        .await
        .unwrap();
    IndexerManager::process_job(
        &pool,
        &job(&phantom, "bafyphantom", &old_rev, WriteAction::Create, None),
        false,
    )
    .await
    .unwrap();
    let pds = MockPds::serving(
        did,
        repo.car().await,
        &frontier(did, &repo.root(), &rev, true),
    )
    .await;

    let dry = reconcile(
        &pool,
        &pds.client,
        &ReconcileOptions {
            dry_run: true,
            ..options(did)
        },
    )
    .await
    .unwrap();
    assert_eq!(dry.phantoms_deleted, 1);
    assert_eq!(dry.records_overwritten, 1);
    assert_eq!(dry.records_inserted, 1);
    assert_eq!(dry.branch, Some(Branch::Ordinary));
    assert!(dry.generation.is_none());
    assert_eq!(
        stored_revs(&pool, did).await.len(),
        2,
        "dry run wrote nothing"
    );

    let report = reconcile(&pool, &pds.client, &options(did)).await.unwrap();
    assert_eq!(report.branch, Some(Branch::Ordinary));
    assert_eq!(report.boundary, rev);
    assert_eq!(report.generation, Some(1));
    assert!(report.refusal.is_none());
    let contents = car_contents(&repo.car().await, did).await.unwrap();
    let mut expected: Vec<(String, String, String)> = contents
        .records
        .iter()
        .map(|(uri, (cid, _))| (uri.clone(), cid.clone(), rev.clone()))
        .collect();
    expected.sort();
    assert_eq!(stored_revs(&pool, did).await, expected);
    let client = pool.get().await.unwrap();
    let fenced: i64 = client
        .query_one(
            "SELECT count(*) FROM wintermute.reconcile_fence WHERE did = $1",
            &[&did],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(fenced, 0, "the fence is released on the ordinary branch");
    let step: String = client
        .query_one(
            "SELECT step FROM wintermute.reconcile_journal WHERE did = $1",
            &[&did],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(step, "done");

    // afterwards, work at or below the boundary is refused for deletes and
    // upserts alike, and work from the old generation is refused above it
    let old_delete = job(&repo.uri("aaa"), "", &rev, WriteAction::Delete, Some(1));
    IndexerManager::process_job(&pool, &old_delete, false)
        .await
        .unwrap();
    assert_eq!(stored_revs(&pool, did).await.len(), 2);
    let newer = format!("{}z", &rev[..rev.len() - 1]);
    let stale_generation = job(
        &repo.uri("ccc"),
        "bafynew",
        &newer,
        WriteAction::Create,
        Some(0),
    );
    let refused = IndexerManager::process_job(&pool, &stale_generation, false)
        .await
        .unwrap_err();
    assert!(matches!(
        refused,
        crate::types::WintermuteError::StaleGeneration(_)
    ));
    let current = job(
        &repo.uri("ccc"),
        "bafynew",
        &newer,
        WriteAction::Create,
        Some(1),
    );
    IndexerManager::process_job(&pool, &current, false)
        .await
        .unwrap();
    assert_eq!(stored_revs(&pool, did).await.len(), 3);

    // the newer record is downstream history the repository does not
    // explain: refused without the flag, removed with it, and only the
    // reconciliation that changes state bumps the generation
    let again = reconcile(&pool, &pds.client, &options(did)).await.unwrap();
    assert!(again.generation.is_none());
    assert!(
        matches!(again.refusal, Some(Refusal::NewerDownstream { ref uris }) if uris == &[repo.uri("ccc")]),
        "{again:?}"
    );
    let reset = reconcile(
        &pool,
        &pds.client,
        &ReconcileOptions {
            reset_to_repo: true,
            ..options(did)
        },
    )
    .await
    .unwrap();
    assert_eq!(reset.generation, Some(2));
    assert_eq!(reset.phantoms_deleted, 1);
    assert_eq!(stored_revs(&pool, did).await.len(), 2);
    drop(pds);
    reset_actor(&pool, did).await;
}

#[tokio::test]
async fn incomplete_history_fails_closed_unless_break_glass() {
    let pool = pool().await;
    let did = "did:plc:wintermute-incomplete";
    reset_actor(&pool, did).await;
    let repo = TestRepo::create(did, &[("aaa", "one")]).await;
    let rev = repo.rev();
    let pds = MockPds::serving(
        did,
        repo.car().await,
        &frontier(did, &repo.root(), &rev, false),
    )
    .await;
    let refused = reconcile(&pool, &pds.client, &options(did)).await.unwrap();
    assert!(matches!(
        refused.refusal,
        Some(Refusal::HistoryIncomplete { reason: Some(ref reason) }) if reason == "test"
    ));
    let dry = reconcile(
        &pool,
        &pds.client,
        &ReconcileOptions {
            dry_run: true,
            ..options(did)
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        dry.refusal,
        Some(Refusal::HistoryIncomplete { .. })
    ));
    assert!(refused.branch.is_none());
    let client = pool.get().await.unwrap();
    assert!(
        super::workflow::persisted_boundary(&client, did)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(stored_revs(&pool, did).await.len(), 0);

    // break-glass reconciles but installs no boundary and leaves the
    // obligation open
    let report = reconcile(
        &pool,
        &pds.client,
        &ReconcileOptions {
            break_glass: true,
            ..options(did)
        },
    )
    .await
    .unwrap();
    assert_eq!(report.records_inserted, 1);
    assert_eq!(
        report.obligation.as_deref(),
        Some("history-incomplete-accepted")
    );
    assert!(report.generation.is_none());
    assert!(
        super::workflow::persisted_boundary(&client, did)
            .await
            .unwrap()
            .is_none()
    );
    let obligation: Option<String> = client
        .query_one(
            "SELECT obligation FROM wintermute.reconcile_journal WHERE did = $1",
            &[&did],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(obligation.as_deref(), Some("history-incomplete-accepted"));
    drop(pds);
    reset_actor(&pool, did).await;
}

#[tokio::test]
async fn newer_downstream_needs_reset_to_repo() {
    let pool = pool().await;
    let did = "did:plc:wintermute-newer";
    reset_actor(&pool, did).await;
    let repo = TestRepo::create(did, &[("aaa", "one"), ("bbb", "two")]).await;
    let rev = repo.rev();
    let newer = format!("{}z", &rev[..rev.len() - 1]);
    // downstream holds a phantom and a differing value both newer than the
    // repository, and the frontier says a newer revision was published, so
    // the missing record bbb may be a newer deletion
    IndexerManager::process_job(
        &pool,
        &job(
            &repo.uri("aaa"),
            "bafynewer",
            &newer,
            WriteAction::Create,
            None,
        ),
        false,
    )
    .await
    .unwrap();
    IndexerManager::process_job(
        &pool,
        &job(
            &repo.uri("zzz"),
            "bafyphantom",
            &newer,
            WriteAction::Create,
            None,
        ),
        false,
    )
    .await
    .unwrap();
    let pds = MockPds::serving(
        did,
        repo.car().await,
        &frontier(did, &repo.root(), &newer, true),
    )
    .await;
    let refused = reconcile(&pool, &pds.client, &options(did)).await.unwrap();
    let Some(Refusal::NewerDownstream { uris }) = refused.refusal else {
        panic!("{refused:?}");
    };
    assert_eq!(
        uris,
        vec![repo.uri("aaa"), repo.uri("zzz"), repo.uri("bbb")]
    );
    assert_eq!(stored_revs(&pool, did).await.len(), 2);

    let report = reconcile(
        &pool,
        &pds.client,
        &ReconcileOptions {
            reset_to_repo: true,
            ..options(did)
        },
    )
    .await
    .unwrap();
    assert_eq!(report.phantoms_deleted, 1);
    assert_eq!(report.records_overwritten, 1);
    assert_eq!(report.records_inserted, 1);
    assert_eq!(report.boundary, newer);
    assert_eq!(report.branch, Some(Branch::Recovery));
    let contents = car_contents(&repo.car().await, did).await.unwrap();
    let (cid_a, _) = &contents.records[&repo.uri("aaa")];
    let (cid_b, _) = &contents.records[&repo.uri("bbb")];
    assert_eq!(
        stored_revs(&pool, did).await,
        vec![
            (repo.uri("aaa"), cid_a.clone(), rev.clone()),
            (repo.uri("bbb"), cid_b.clone(), rev.clone())
        ]
    );
    // the recovery branch releases the application fence and waits for the
    // acknowledgement of a commit above the boundary
    let client = pool.get().await.unwrap();
    let step: String = client
        .query_one(
            "SELECT step FROM wintermute.reconcile_journal WHERE did = $1",
            &[&did],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(step, "awaiting-recovery");
    let pending = verify_recovery(&pool, did, "bafyrecovery").await.unwrap();
    assert!(!pending.acknowledged);
    let recovery = IndexJob {
        uri: format!("at://{did}"),
        cid: "bafyrecovery".to_owned(),
        action: WriteAction::Commit,
        record: None,
        indexed_at: "2026-01-01T00:00:00.000Z".to_owned(),
        rev: format!("{newer}0"),
        provenance: Some(Provenance {
            generation: Some(1),
            source: Source::Firehose { seq: 9 },
        }),
    };
    IndexerManager::process_job(&pool, &recovery, false)
        .await
        .unwrap();
    let acknowledged = verify_recovery(&pool, did, "bafyrecovery").await.unwrap();
    assert!(acknowledged.acknowledged);
    assert_eq!(
        acknowledged.last_commit_rev.as_deref(),
        Some(format!("{newer}0").as_str())
    );
    let step: String = client
        .query_one(
            "SELECT step FROM wintermute.reconcile_journal WHERE did = $1",
            &[&did],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(step, "done");
    drop(pds);
    reset_actor(&pool, did).await;
}

#[tokio::test]
async fn an_export_that_does_not_match_the_frontier_is_refused() {
    let pool = pool().await;
    let did = "did:plc:wintermute-mismatch";
    reset_actor(&pool, did).await;
    let repo = TestRepo::create(did, &[("aaa", "one")]).await;
    let mut wrong = frontier(did, &repo.root(), &repo.rev(), true);
    wrong.current_commit_cid = Some("bafyelsewhere".to_owned());
    let pds = MockPds::serving(did, repo.car().await, &wrong).await;
    let refused = reconcile(&pool, &pds.client, &options(did)).await.unwrap();
    assert!(matches!(
        refused.refusal,
        Some(Refusal::RootMismatch { .. })
    ));
    // a wrong actor's export is an error, not a plan
    let other = TestRepo::create("did:plc:wintermute-other", &[("aaa", "one")]).await;
    let err = car_contents(&other.car().await, did).await.unwrap_err();
    assert!(err.to_string().contains("did mismatch"), "{err}");
    assert!(car_contents(b"not a car", did).await.is_err());
    drop(pds);
    reset_actor(&pool, did).await;
}

#[tokio::test]
async fn fenced_jobs_are_deferred_in_batches_and_applied_afterwards() {
    let pool = pool().await;
    let did = "did:plc:wintermute-fenced";
    reset_actor(&pool, did).await;
    let client = pool.get().await.unwrap();
    client
        .execute(
            "INSERT INTO wintermute.reconcile_fence (did, workflow_id) VALUES ($1, 'wf-x')",
            &[&did],
        )
        .await
        .unwrap();
    let jobs = vec![
        (
            b"k1".to_vec(),
            job(
                &format!("at://{did}/com.example.note/aaa"),
                "bafya",
                "3a",
                WriteAction::Create,
                None,
            ),
        ),
        (
            b"k3".to_vec(),
            job(
                &format!("at://{did}/com.example.note/bbb"),
                "bafyb",
                "3b",
                WriteAction::Create,
                None,
            ),
        ),
        (
            b"k2".to_vec(),
            IndexJob {
                uri: format!("at://{did}"),
                cid: "bafycommit".to_owned(),
                action: WriteAction::Commit,
                record: None,
                indexed_at: "2026-01-01T00:00:00.000Z".to_owned(),
                rev: "3a".to_owned(),
                provenance: None,
            },
        ),
    ];
    let (results, _) = IndexerManager::process_jobs_batch(&pool, &jobs, false, false).await;
    assert!(
        results
            .iter()
            .all(|(_, result)| matches!(result, Err(crate::types::WintermuteError::Fenced(_))))
    );
    assert!(stored_revs(&pool, did).await.is_empty());
    client
        .execute(
            "DELETE FROM wintermute.reconcile_fence WHERE did = $1",
            &[&did],
        )
        .await
        .unwrap();
    let (results, failed) = IndexerManager::process_jobs_batch(&pool, &jobs, false, false).await;
    assert!(!failed);
    assert!(results.iter().all(|(_, result)| result.is_ok()));
    assert_eq!(stored_revs(&pool, did).await.len(), 2);
    let progress = progress_of(&client, did).await.unwrap().unwrap();
    assert_eq!(progress.last_commit_cid.as_deref(), Some("bafycommit"));
    assert_eq!(progress.max_rev, "3b");
    reset_actor(&pool, did).await;
}

#[tokio::test]
async fn a_multi_revision_repository_lists_its_current_records() {
    let did = "did:plc:wintermute-revisions";
    let mut repo = TestRepo::create(did, &[("aaa", "one"), ("bbb", "two")]).await;
    let first = repo.rev();
    repo.write(vec![
        RecordWriteOp::Delete(RecordDeleteOp {
            action: WriteOpAction::Delete,
            collection: "com.example.note".to_owned(),
            rkey: "bbb".to_owned(),
        }),
        RecordWriteOp::Create(note("ccc", "three")),
    ])
    .await;
    assert!(repo.rev() > first);
    let contents = car_contents(&repo.car().await, did).await.unwrap();
    assert_eq!(contents.rev, repo.rev());
    let mut uris: Vec<&String> = contents.records.keys().collect();
    uris.sort();
    assert_eq!(uris, vec![&repo.uri("aaa"), &repo.uri("ccc")]);
    assert_eq!(contents.records[&repo.uri("ccc")].1["text"], "three");
}

#[tokio::test]
async fn an_unreachable_frontier_fails_closed_and_releases_the_fence() {
    let pool = pool().await;
    let did = "did:plc:wintermute-unreachable";
    reset_actor(&pool, did).await;
    let repo = TestRepo::create(did, &[("aaa", "one")]).await;
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/xrpc/com.atproto.sync.getRepo")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(repo.car().await)
        .create_async()
        .await;
    server
        .mock("GET", "/xrpc/community.blacksky.pds.getPublicationFrontier")
        .match_query(mockito::Matcher::Any)
        .with_status(503)
        .create_async()
        .await;
    let client = FrontierClient::new(&server.url(), "secret").unwrap();
    let err = reconcile(&pool, &client, &options(did)).await.unwrap_err();
    assert!(err.to_string().contains("503"), "{err}");
    drop(server);
    let db = pool.get().await.unwrap();
    let fenced: i64 = db
        .query_one(
            "SELECT count(*) FROM wintermute.reconcile_fence WHERE did = $1",
            &[&did],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(fenced, 0);
    assert!(
        super::workflow::persisted_boundary(&db, did)
            .await
            .unwrap()
            .is_none()
    );
    // an export the PDS cannot serve is an error as well
    let mut empty = mockito::Server::new_async().await;
    empty
        .mock("GET", "/xrpc/com.atproto.sync.getRepo")
        .match_query(mockito::Matcher::Any)
        .with_status(404)
        .create_async()
        .await;
    let client = FrontierClient::new(&empty.url(), "secret").unwrap();
    assert!(client.repo_car(did).await.is_err());
    assert!(client.frontier(did).await.is_err());
    drop(empty);
    // a database that cannot be reached fails every job of a batch and a
    // single job alike, and an empty batch is nothing
    let unreachable = crate::config::create_pg_pool(
        "postgresql://postgres:postgres@127.0.0.1:1/nowhere",
        crate::config::pg_pool_config(1),
    )
    .unwrap();
    let jobs = vec![(
        b"k".to_vec(),
        job(
            &format!("at://{did}/com.example.note/aaa"),
            "bafya",
            "3a",
            WriteAction::Create,
            None,
        ),
    )];
    let (results, failed) =
        IndexerManager::process_jobs_batch(&unreachable, &jobs, false, false).await;
    assert!(failed);
    assert!(results.iter().all(|(_, result)| result.is_err()));
    let (results, failed) =
        IndexerManager::process_jobs_batch(&unreachable, &[], false, false).await;
    assert!(!failed && results.is_empty());
    assert!(
        IndexerManager::process_job(&unreachable, &jobs[0].1, false)
            .await
            .is_err()
    );
    reset_actor(&pool, did).await;
}
