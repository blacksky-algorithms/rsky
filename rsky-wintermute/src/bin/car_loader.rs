//! Bulk-load a CAR export into the bsky Postgres schema.
//!
//! Reads per-repo `.car` files listed in a SQLite manifest, parses them in parallel, and writes
//! records in cross-repo COPY batches. Resumable via an on-disk SQLite state file. Aggregates are
//! recomputed separately afterward, so writes run in bulk-load mode (no inline aggregates).

use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use clap::Parser;
use color_eyre::Result;
use deadpool_postgres::{Config, ManagerConfig, Pool, RecyclingMethod, Runtime};
use iroh_car::CarReader;
use rsky_repo::parse::get_and_parse_record;
use rsky_repo::readable_repo::ReadableRepo;
use rsky_repo::storage::memory_blockstore::MemoryBlockstore;
use rsky_syntax::aturi::AtUri;
use rusqlite::OpenFlags;
use tokio_postgres::NoTls;

use rsky_wintermute::backfiller::convert_record_to_ipld;
use rsky_wintermute::config::ingest_collection_allowed;
use rsky_wintermute::indexer::IndexerManager;
use rsky_wintermute::types::{IndexJob, WriteAction};

#[derive(Debug, Parser)]
#[command(name = "car_loader")]
#[command(about = "Bulk-load a CAR export into the bsky Postgres schema")]
struct Args {
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,

    #[arg(long, env = "CAR_DUMP_DIR", default_value = "/data/car-dump")]
    car_dump_dir: PathBuf,

    /// SQLite manifest; defaults to {car_dump_dir}/car-dump.sqlite
    #[arg(long, env = "CAR_DUMP_SQLITE")]
    manifest: Option<PathBuf>,

    /// On-disk loader state (resume + failures); kept beside the binary by default.
    #[arg(
        long,
        env = "CAR_LOADER_STATE",
        default_value = "./car_loader_state.sqlite"
    )]
    state_file: PathBuf,

    #[arg(long, env = "CAR_LOADER_WORKERS", default_value_t = num_cpus::get())]
    workers: usize,

    #[arg(long, env = "CAR_LOADER_WRITERS", default_value_t = 24)]
    writers: usize,

    /// Records accumulated across repos before a COPY flush.
    #[arg(long, env = "CAR_LOADER_BATCH", default_value_t = 20_000)]
    batch_size: usize,

    #[arg(long, env = "CAR_LOADER_SHARDS", default_value_t = 1)]
    shards: i64,

    #[arg(long, env = "CAR_LOADER_SHARD", default_value_t = 0)]
    shard: i64,

    #[arg(long, env = "DB_POOL_SIZE", default_value_t = 48)]
    db_pool_size: usize,

    /// Process a single repo with verbose logging (for verifying a failure case).
    #[arg(long = "did")]
    did: Option<String>,

    /// Reprocess only the DIDs recorded in the state file's `failed` table.
    #[arg(long, default_value_t = false)]
    retry_failed: bool,

    /// Print progress (percent complete, rates) from the state file and exit.
    #[arg(long, default_value_t = false)]
    status: bool,
}

impl Args {
    fn manifest_path(&self) -> PathBuf {
        self.manifest
            .clone()
            .unwrap_or_else(|| self.car_dump_dir.join("car-dump.sqlite"))
    }
}

/// One repo to load, taken from the manifest.
#[derive(Clone)]
struct ManifestRow {
    did: String,
    path: String,
}

/// A parsed repo: index jobs to write plus any per-record extraction failures.
struct ParsedRepo {
    did: String,
    jobs: Vec<IndexJob>,
    record_failures: Vec<RecordFailure>,
}

struct RecordFailure {
    uri: String,
    collection: String,
    error: String,
}

/// Messages to the single thread that owns the state SQLite.
enum StateMsg {
    ReposDone(Vec<String>),
    RepoFailed {
        did: String,
        path: String,
        error: String,
    },
    RecordsFailed(Vec<RecordFailure>),
    Sample,
}

#[derive(Default)]
struct Counters {
    repos_done: AtomicU64,
    repos_failed: AtomicU64,
    records_written: AtomicU64,
    records_failed: AtomicU64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    color_eyre::install()?;
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    if args.status {
        return report_status(&args);
    }

    let pool = build_pg_pool(&args)?;

    if let Some(did) = args.did.clone() {
        return load_single_repo(&args, &pool, &did).await;
    }

    run_full_load(args, pool).await
}

/// Build the Postgres connection pool.
fn build_pg_pool(args: &Args) -> Result<Pool> {
    let mut cfg = Config::new();
    cfg.url = Some(args.database_url.clone());
    cfg.pool = Some(deadpool_postgres::PoolConfig::new(args.db_pool_size));
    cfg.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });
    Ok(cfg.create_pool(Some(Runtime::Tokio1), NoTls)?)
}

/// Read a repo's CAR, walk its MST, and build index jobs for allowed collections.
async fn parse_repo_to_jobs(
    car_dump_dir: &std::path::Path,
    row: &ManifestRow,
) -> Result<ParsedRepo> {
    let full_path = car_dump_dir.join(&row.path);
    let bytes = tokio::task::spawn_blocking(move || std::fs::read(full_path)).await??;

    let mut reader = CarReader::new(Cursor::new(bytes))
        .await
        .map_err(|e| color_eyre::eyre::eyre!("CAR parse: {e}"))?;
    let root = *reader
        .header()
        .roots()
        .first()
        .ok_or_else(|| color_eyre::eyre::eyre!("no root CID"))?;
    let mut blocks = rsky_repo::block_map::BlockMap::new();
    while let Some((cid, data)) = reader
        .next_block()
        .await
        .map_err(|e| color_eyre::eyre::eyre!("CAR block: {e}"))?
    {
        blocks.set(cid, data);
    }

    let blockstore = MemoryBlockstore::new(Some(blocks))
        .await
        .map_err(|e| color_eyre::eyre::eyre!("blockstore: {e}"))?;
    let storage = Arc::new(tokio::sync::RwLock::new(blockstore));
    let mut repo = ReadableRepo::load(storage, root)
        .await
        .map_err(|e| color_eyre::eyre::eyre!("repo load: {e}"))?;

    if repo.did() != &row.did {
        return Err(color_eyre::eyre::eyre!(
            "DID mismatch: file {} vs commit {}",
            row.did,
            repo.did()
        ));
    }

    let leaves = repo
        .data
        .list(None, None, None)
        .await
        .map_err(|e| color_eyre::eyre::eyre!("mst list: {e}"))?;
    let block_data = {
        let guard = repo.storage.read().await;
        guard
            .get_blocks(leaves.iter().map(|e| e.value).collect())
            .await
            .map_err(|e| color_eyre::eyre::eyre!("get_blocks: {e}"))?
    };

    let rev = repo.commit.rev.clone();
    let now = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();

    let did = row.did.clone();
    let mut jobs = Vec::with_capacity(leaves.len());
    let mut record_failures = Vec::new();

    for entry in &leaves {
        let Ok(uri) = AtUri::new(format!("at://{did}/{}", entry.key), None) else {
            continue;
        };
        let collection = uri.get_collection();
        if !ingest_collection_allowed(&collection) {
            continue;
        }
        // A malformed record is logged and skipped, never allowed to fail the whole repo.
        match get_and_parse_record(&block_data.blocks, entry.value) {
            Ok(parsed) => match serde_json::to_value(&parsed.record) {
                Ok(raw) => jobs.push(IndexJob {
                    uri: uri.to_string(),
                    cid: entry.value.to_string(),
                    action: WriteAction::Create,
                    record: Some(convert_record_to_ipld(&raw)),
                    indexed_at: now.clone(),
                    rev: rev.clone(),
                }),
                Err(e) => record_failures.push(RecordFailure {
                    uri: uri.to_string(),
                    collection,
                    error: format!("to_value: {e}"),
                }),
            },
            Err(e) => record_failures.push(RecordFailure {
                uri: uri.to_string(),
                collection,
                error: format!("parse: {e}"),
            }),
        }
    }

    Ok(ParsedRepo {
        did,
        jobs,
        record_failures,
    })
}

/// Run the full load: enqueue repos from the manifest, parse them in worker tasks, and write the
/// resulting records to Postgres in cross-repo batches.
async fn run_full_load(args: Args, pool: Pool) -> Result<()> {
    let args = Arc::new(args);
    let counters = Arc::new(Counters::default());

    let (state_tx, state_rx) = std::sync::mpsc::channel::<StateMsg>();
    let state_path = args.state_file.clone();
    let state_counters = Arc::clone(&counters);
    let state_thread =
        std::thread::spawn(move || run_state_store(&state_path, &state_rx, &state_counters));

    let (work_tx, work_rx) = tokio::sync::mpsc::channel::<ManifestRow>(args.workers * 4);
    spawn_repo_enqueuer(&args, work_tx);

    let (parsed_tx, parsed_rx) = tokio::sync::mpsc::channel::<ParsedRepo>(args.writers * 4);
    let work_rx = Arc::new(tokio::sync::Mutex::new(work_rx));
    let parsed_rx = Arc::new(tokio::sync::Mutex::new(parsed_rx));

    let mut worker_handles = Vec::new();
    for _ in 0..args.workers {
        let work_rx = Arc::clone(&work_rx);
        let parsed_tx = parsed_tx.clone();
        let state_tx = state_tx.clone();
        let args = Arc::clone(&args);
        let counters = Arc::clone(&counters);
        worker_handles.push(tokio::spawn(async move {
            loop {
                let row = {
                    let mut rx = work_rx.lock().await;
                    rx.recv().await
                };
                let Some(row) = row else { break };
                // Isolate parsing in a child task so a panic in rsky-repo (e.g. an
                // invalid MST key) is recorded as a repo failure instead of killing
                // the worker, which would otherwise deplete the pool and stall.
                let cdd = args.car_dump_dir.clone();
                let row_for_parse = row.clone();
                let parse_result =
                    tokio::spawn(async move { parse_repo_to_jobs(&cdd, &row_for_parse).await })
                        .await;
                match parse_result {
                    Ok(Ok(mut parsed)) => {
                        let failures = std::mem::take(&mut parsed.record_failures);
                        if !failures.is_empty() {
                            counters
                                .records_failed
                                .fetch_add(failures.len() as u64, Ordering::Relaxed);
                            let _ = state_tx.send(StateMsg::RecordsFailed(failures));
                        }
                        let _ = parsed_tx.send(parsed).await;
                    }
                    Ok(Err(e)) => {
                        counters.repos_failed.fetch_add(1, Ordering::Relaxed);
                        let _ = state_tx.send(StateMsg::RepoFailed {
                            did: row.did.clone(),
                            path: row.path.clone(),
                            error: format!("{e:#}"),
                        });
                    }
                    Err(join_err) => {
                        counters.repos_failed.fetch_add(1, Ordering::Relaxed);
                        let _ = state_tx.send(StateMsg::RepoFailed {
                            did: row.did.clone(),
                            path: row.path.clone(),
                            error: format!("parse panicked: {join_err}"),
                        });
                    }
                }
            }
        }));
    }
    drop(parsed_tx);

    let mut writer_handles = Vec::new();
    for _ in 0..args.writers {
        let parsed_rx = Arc::clone(&parsed_rx);
        let pool = pool.clone();
        let state_tx = state_tx.clone();
        let counters = Arc::clone(&counters);
        let batch_size = args.batch_size;
        writer_handles.push(tokio::spawn(async move {
            let mut batch_jobs: Vec<(Vec<u8>, IndexJob)> = Vec::new();
            let mut batch_dids: Vec<String> = Vec::new();
            loop {
                let parsed = {
                    let mut rx = parsed_rx.lock().await;
                    rx.recv().await
                };
                let Some(parsed) = parsed else { break };
                for job in parsed.jobs {
                    batch_jobs.push((job.uri.clone().into_bytes(), job));
                }
                batch_dids.push(parsed.did);
                if batch_jobs.len() >= batch_size {
                    write_batch(
                        &pool,
                        &mut batch_jobs,
                        &mut batch_dids,
                        &state_tx,
                        &counters,
                    )
                    .await;
                }
            }
            write_batch(
                &pool,
                &mut batch_jobs,
                &mut batch_dids,
                &state_tx,
                &counters,
            )
            .await;
        }));
    }
    drop(state_tx);

    let logger = spawn_progress_logger(Arc::clone(&counters));

    for h in worker_handles {
        let _ = h.await;
    }
    for h in writer_handles {
        let _ = h.await;
    }
    logger.abort();
    let _ = state_thread.join();

    tracing::info!(
        "done: repos done={}, failed={}, records={}, record failures={}",
        counters.repos_done.load(Ordering::Relaxed),
        counters.repos_failed.load(Ordering::Relaxed),
        counters.records_written.load(Ordering::Relaxed),
        counters.records_failed.load(Ordering::Relaxed),
    );
    Ok(())
}

/// Write one accumulated cross-repo batch to Postgres and mark its repos done.
async fn write_batch(
    pool: &Pool,
    batch_jobs: &mut Vec<(Vec<u8>, IndexJob)>,
    batch_dids: &mut Vec<String>,
    state_tx: &std::sync::mpsc::Sender<StateMsg>,
    counters: &Counters,
) {
    if batch_jobs.is_empty() {
        // Repos with no persistable records are still done (nothing to write).
        if !batch_dids.is_empty() {
            counters
                .repos_done
                .fetch_add(batch_dids.len() as u64, Ordering::Relaxed);
            let _ = state_tx.send(StateMsg::ReposDone(std::mem::take(batch_dids)));
            let _ = state_tx.send(StateMsg::Sample);
        }
        return;
    }

    let n = batch_jobs.len() as u64;
    // bulk_load = true: skip inline aggregates + the like-insert semaphore.
    let (_results, batch_failed) = IndexerManager::process_jobs_batch(pool, batch_jobs, true).await;
    batch_jobs.clear();

    if batch_failed {
        // Rows were not persisted: leave these repos NOT done so a resume re-processes them
        // (from the manifest, with the correct path) instead of silently dropping them.
        let repos = batch_dids.len() as u64;
        counters.repos_failed.fetch_add(repos, Ordering::Relaxed);
        tracing::warn!("batch copy failed; {repos} repos left not-done for resume");
        batch_dids.clear();
        return;
    }

    counters.records_written.fetch_add(n, Ordering::Relaxed);
    counters
        .repos_done
        .fetch_add(batch_dids.len() as u64, Ordering::Relaxed);
    let _ = state_tx.send(StateMsg::ReposDone(std::mem::take(batch_dids)));
    let _ = state_tx.send(StateMsg::Sample);
}

/// Log throughput (records/s, repos/s, percent via the state file) every 30s.
fn spawn_progress_logger(counters: Arc<Counters>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let start = Instant::now();
        let mut last = (0u64, 0u64, Instant::now());
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            let done = counters.repos_done.load(Ordering::Relaxed);
            let failed = counters.repos_failed.load(Ordering::Relaxed);
            let recs = counters.records_written.load(Ordering::Relaxed);
            let dt = last.2.elapsed().as_secs_f64().max(1.0);
            let rps = (recs.saturating_sub(last.0)) as f64 / dt;
            let repos_ps = (done.saturating_sub(last.1)) as f64 / dt;
            tracing::info!(
                "progress: repos done={done} failed={failed}, records={recs}, {rps:.0} rec/s, {repos_ps:.0} repos/s, elapsed {}s",
                start.elapsed().as_secs()
            );
            last = (recs, done, Instant::now());
        }
    })
}

/// Spawn the manifest reader on a blocking thread (rusqlite is synchronous).
fn spawn_repo_enqueuer(args: &Arc<Args>, work_tx: tokio::sync::mpsc::Sender<ManifestRow>) {
    let args = Arc::clone(args);
    std::thread::spawn(move || {
        if let Err(e) = enqueue_repos(&args, &work_tx) {
            tracing::error!("manifest reader failed: {e:#}");
        }
    });
}

/// Stream manifest rows (skipping already-done DIDs) into the work channel; or the recorded
/// failures when `--retry-failed`.
fn enqueue_repos(args: &Args, work_tx: &tokio::sync::mpsc::Sender<ManifestRow>) -> Result<()> {
    let manifest = rusqlite::Connection::open_with_flags(
        args.manifest_path(),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    // Read-write so a fresh run can create the table; reads the prior run's `done` for resume.
    let state = rusqlite::Connection::open(&args.state_file)?;
    state.execute_batch("CREATE TABLE IF NOT EXISTS done (did TEXT PRIMARY KEY)")?;

    if args.retry_failed {
        let mut stmt = state.prepare("SELECT did, path FROM failed")?;
        let rows = stmt.query_map([], manifest_row)?;
        for row in rows {
            if work_tx.blocking_send(row?).is_err() {
                break;
            }
        }
        return Ok(());
    }

    // Load done DIDs into memory once; an O(1) skip avoids millions of per-row SQLite lookups.
    let mut done: std::collections::HashSet<String> = std::collections::HashSet::new();
    {
        let mut stmt = state.prepare("SELECT did FROM done")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for did in rows {
            done.insert(did?);
        }
    }

    let mut stmt = manifest.prepare(
        "SELECT did, path FROM repo_dumps \
         WHERE (?1 = 1 OR rowid % ?1 = ?2) ORDER BY did",
    )?;
    let rows = stmt.query_map(rusqlite::params![args.shards, args.shard], manifest_row)?;
    for row in rows {
        let row = row?;
        if done.contains(&row.did) {
            continue;
        }
        if work_tx.blocking_send(row).is_err() {
            break;
        }
    }
    Ok(())
}

fn manifest_row(r: &rusqlite::Row) -> rusqlite::Result<ManifestRow> {
    Ok(ManifestRow {
        did: r.get(0)?,
        path: r.get(1)?,
    })
}

/// Owns the state SQLite on a single thread: records done/failed repos, failed records, and
/// periodic progress samples.
fn run_state_store(
    path: &std::path::Path,
    rx: &std::sync::mpsc::Receiver<StateMsg>,
    counters: &Counters,
) {
    let conn = match rusqlite::Connection::open(path) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("state db open failed: {e}");
            return;
        }
    };
    let _ = conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE IF NOT EXISTS done (did TEXT PRIMARY KEY);
         CREATE TABLE IF NOT EXISTS failed (did TEXT PRIMARY KEY, path TEXT, error TEXT, attempted_at TEXT DEFAULT (datetime('now')));
         CREATE TABLE IF NOT EXISTS failed_records (uri TEXT PRIMARY KEY, collection TEXT, error TEXT, attempted_at TEXT DEFAULT (datetime('now')));
         CREATE TABLE IF NOT EXISTS progress (id INTEGER PRIMARY KEY CHECK (id=1), repos_done INTEGER, repos_failed INTEGER, records_written INTEGER, updated_at TEXT);
         CREATE TABLE IF NOT EXISTS progress_samples (ts TEXT, records_written INTEGER, repos_done INTEGER);",
    );

    while let Ok(msg) = rx.recv() {
        match msg {
            StateMsg::ReposDone(dids) => {
                if let Ok(tx) = conn.unchecked_transaction() {
                    if let Ok(mut stmt) =
                        tx.prepare_cached("INSERT OR IGNORE INTO done (did) VALUES (?1)")
                    {
                        for did in &dids {
                            let _ = stmt.execute([did]);
                        }
                    }
                    let _ = tx.commit();
                }
            }
            StateMsg::RepoFailed { did, path, error } => {
                let _ = conn.execute(
                    "INSERT OR REPLACE INTO failed (did, path, error) VALUES (?1, ?2, ?3)",
                    rusqlite::params![did, path, error],
                );
            }
            StateMsg::RecordsFailed(failures) => {
                if let Ok(tx) = conn.unchecked_transaction() {
                    if let Ok(mut stmt) = tx.prepare_cached(
                        "INSERT OR IGNORE INTO failed_records (uri, collection, error) VALUES (?1, ?2, ?3)",
                    ) {
                        for f in &failures {
                            let _ = stmt.execute(rusqlite::params![f.uri, f.collection, f.error]);
                        }
                    }
                    let _ = tx.commit();
                }
            }
            StateMsg::Sample => {
                let done = counters.repos_done.load(Ordering::Relaxed) as i64;
                let failed = counters.repos_failed.load(Ordering::Relaxed) as i64;
                let recs = counters.records_written.load(Ordering::Relaxed) as i64;
                let _ = conn.execute(
                    "INSERT INTO progress (id, repos_done, repos_failed, records_written, updated_at)
                     VALUES (1, ?1, ?2, ?3, datetime('now'))
                     ON CONFLICT (id) DO UPDATE SET repos_done=?1, repos_failed=?2, records_written=?3, updated_at=datetime('now')",
                    rusqlite::params![done, failed, recs],
                );
                let _ = conn.execute(
                    "INSERT INTO progress_samples (ts, records_written, repos_done) VALUES (datetime('now'), ?1, ?2)",
                    rusqlite::params![recs, done],
                );
            }
        }
    }
}

/// Print percent-complete and recent rate from the state file (safe to run during a load).
fn report_status(args: &Args) -> Result<()> {
    let manifest = rusqlite::Connection::open_with_flags(
        args.manifest_path(),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let total: i64 = manifest.query_row("SELECT count(*) FROM repo_dumps", [], |r| r.get(0))?;

    let state =
        rusqlite::Connection::open_with_flags(&args.state_file, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let done: i64 = state
        .query_row("SELECT count(*) FROM done", [], |r| r.get(0))
        .unwrap_or(0);
    let failed: i64 = state
        .query_row("SELECT count(*) FROM failed", [], |r| r.get(0))
        .unwrap_or(0);
    let recs: i64 = state
        .query_row("SELECT records_written FROM progress WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap_or(0);

    let mut rate = String::from("n/a");
    let mut stmt =
        state.prepare("SELECT records_written FROM progress_samples ORDER BY ts DESC LIMIT 2")?;
    let samples: Vec<i64> = stmt
        .query_map([], |r| r.get(0))?
        .filter_map(|x| x.ok())
        .collect();
    if samples.len() == 2 {
        rate = format!("~{} records / 30s window", samples[0] - samples[1]);
    }

    let pct = if total > 0 {
        (done + failed) as f64 * 100.0 / total as f64
    } else {
        0.0
    };
    println!("CAR loader status:");
    println!("  repos: {done} done + {failed} failed / {total} ({pct:.2}%)");
    println!("  records written: {recs}");
    println!("  rate: {rate}");
    Ok(())
}

/// Parse and write a single repo with verbose logging (for verifying one DID).
async fn load_single_repo(args: &Args, pool: &Pool, did: &str) -> Result<()> {
    let manifest = rusqlite::Connection::open_with_flags(
        args.manifest_path(),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let row = manifest.query_row(
        "SELECT did, path FROM repo_dumps WHERE did = ?1",
        [did],
        manifest_row,
    )?;
    tracing::info!("processing {did} from {}", row.path);
    let parsed = parse_repo_to_jobs(&args.car_dump_dir, &row).await?;
    tracing::info!(
        "parsed {} records, {} record failures",
        parsed.jobs.len(),
        parsed.record_failures.len()
    );
    for f in &parsed.record_failures {
        tracing::warn!("record failure {}: {}", f.uri, f.error);
    }
    let mut jobs: Vec<(Vec<u8>, IndexJob)> = parsed
        .jobs
        .into_iter()
        .map(|j| (j.uri.clone().into_bytes(), j))
        .collect();
    let (results, batch_failed) = IndexerManager::process_jobs_batch(pool, &jobs, true).await;
    let errs = results.iter().filter(|(_, r)| r.is_err()).count();
    jobs.clear();
    tracing::info!(
        "wrote {} records ({errs} write errors, batch_failed={batch_failed})",
        results.len()
    );
    Ok(())
}
