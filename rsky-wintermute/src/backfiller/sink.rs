//! Where parsed repos go: straight into Postgres through the bulk COPY path.
//!
//! The old backfiller parsed each archive into an LMDB queue that a separate
//! indexer loop drained. Measured on the profiling box, that hop was the
//! bottleneck: one LMDB write transaction per repo, one writer per environment,
//! ~1,400 records/s with the box 74% idle. The sink here is the `car_loader`
//! design instead -- parsed repos are batched across repos and written with
//! `process_jobs_batch` (COPY into staging, then `INSERT ... ON CONFLICT`),
//! which measured 7,000-9,300 records/s on the same 8 vCPU.
//!
//! The channel into the writers is bounded, and that bound *is* the demand
//! gate: fetch workers ask [`RecordSink::has_capacity`] before claiming work,
//! so nothing is pulled from a PDS or hubble that Postgres is not ready to take.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use deadpool_postgres::Pool;
use tokio::sync::{Mutex, mpsc, oneshot};

use super::ParsedRepo;
use super::source::RepoBody;
use crate::indexer::IndexerManager;
use crate::types::{IndexJob, WintermuteError};

/// What the sink hands back once a repo has been parsed and accepted.
/// `committed` resolves when the batch holding this repo has been written.
#[derive(Debug)]
pub struct Receipt {
    pub records: usize,
    pub filtered: usize,
    pub rev: String,
    pub committed: oneshot::Receiver<Result<(), WintermuteError>>,
}

/// Where parsed records go, and whether there is room for more.
pub trait RecordSink: Send + Sync + 'static {
    /// Whether a worker should claim more repos right now.
    fn has_capacity(&self) -> bool;

    /// Parse an archive and hand its records over. Returns as soon as the
    /// records are accepted for writing; the receipt says when they landed.
    fn ingest(
        &self,
        did: &str,
        body: RepoBody,
    ) -> impl Future<Output = Result<Receipt, WintermuteError>> + Send;

    /// Stop accepting repos and wait for everything already accepted to
    /// commit. Unbounded by design -- a normal drain must not lose records --
    /// so a shutdown wraps it in the grace budget (see
    /// [`super::finish_sink`]).
    fn finish(&self) -> impl Future<Output = ()> + Send {
        async {}
    }

    /// `(repos, records)` accepted but not yet committed.
    fn uncommitted(&self) -> (usize, usize) {
        (0, 0)
    }

    /// Give up on whatever is uncommitted: the writers are stopped and every
    /// pending receipt resolves as dropped. Called once the grace budget has
    /// run out; the repos concerned stay claimed and are re-fetched on
    /// restart.
    fn abandon(&self) -> impl Future<Output = ()> + Send {
        async {}
    }
}

/// A repo waiting for a writer.
struct Pending {
    did: String,
    jobs: Vec<IndexJob>,
    done: oneshot::Sender<Result<(), WintermuteError>>,
}

#[derive(Debug, Clone)]
pub struct PgSinkConfig {
    /// Concurrent COPY writers. Each holds one pool connection while writing
    /// and fans out to a few more inside `process_jobs_batch`.
    pub writers: usize,
    /// Records per COPY batch. Small repos (median 3 KiB) are batched across
    /// repos to reach this; a whale is written on its own.
    pub batch_jobs: usize,
    /// Parsed repos allowed to wait for a writer.
    pub queue_repos: usize,
    /// Records accepted but not yet committed. Parsed records are JSON values
    /// several times the size of their CBOR, so this -- not the repo count --
    /// is what bounds the sink's memory. A whale repo can exceed it on its
    /// own; it is admitted, and everything else waits.
    pub max_records_in_flight: usize,
    /// How long a writer waits for a batch to fill before flushing a partial
    /// one.
    pub flush_after: Duration,
    /// `RECORD_SKIP_BOILERPLATE`, threaded through so tests can pin it.
    pub skip_boilerplate: bool,
}

impl Default for PgSinkConfig {
    fn default() -> Self {
        Self {
            writers: 4,
            batch_jobs: 2000,
            queue_repos: 256,
            max_records_in_flight: 250_000,
            flush_after: Duration::from_millis(500),
            skip_boilerplate: *crate::config::RECORD_SKIP_BOILERPLATE,
        }
    }
}

/// Counters the runner reports and tests assert on.
#[derive(Debug, Default)]
pub struct SinkCounters {
    pub repos_written: AtomicU64,
    pub records_written: AtomicU64,
    pub record_failures: AtomicU64,
    pub batches: AtomicU64,
    pub batches_failed: AtomicU64,
    pub records_in_flight: AtomicUsize,
    pub repos_in_flight: AtomicUsize,
}

pub struct PgSink {
    /// `None` once [`PgSink::finish`] has closed the intake: the writers exit
    /// when the last sender is gone.
    tx: std::sync::Mutex<Option<mpsc::Sender<Pending>>>,
    counters: Arc<SinkCounters>,
    max_records_in_flight: usize,
    writers: Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

impl PgSink {
    /// Spawns the writer tasks on the current runtime. `pool` must come from
    /// `config::create_pg_pool` so its connections carry the staging tables.
    #[must_use]
    pub fn new(pool: &Pool, cfg: &PgSinkConfig) -> Self {
        let (tx, rx) = mpsc::channel::<Pending>(cfg.queue_repos.max(1));
        let rx = Arc::new(Mutex::new(rx));
        let counters = Arc::new(SinkCounters::default());
        let mut writers = Vec::with_capacity(cfg.writers.max(1));
        for id in 0..cfg.writers.max(1) {
            let rx = Arc::clone(&rx);
            let pool = pool.clone();
            let counters = Arc::clone(&counters);
            let cfg = cfg.clone();
            writers.push(tokio::spawn(async move {
                writer_loop(id, rx, pool, cfg, counters).await;
            }));
        }
        Self {
            tx: std::sync::Mutex::new(Some(tx)),
            counters,
            max_records_in_flight: cfg.max_records_in_flight.max(1),
            writers: Mutex::new(writers),
        }
    }

    /// A handle on the intake, or `None` once it is closed.
    fn sender(&self) -> Option<mpsc::Sender<Pending>> {
        self.tx
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    #[must_use]
    pub const fn counters(&self) -> &Arc<SinkCounters> {
        &self.counters
    }

    /// Repos waiting for a writer.
    #[must_use]
    pub fn queued(&self) -> usize {
        self.sender()
            .map_or(0, |tx| tx.max_capacity() - tx.capacity())
    }

    /// Records accepted but not yet committed.
    #[must_use]
    pub fn records_in_flight(&self) -> usize {
        self.counters.records_in_flight.load(Ordering::Relaxed)
    }
}

impl RecordSink for PgSink {
    fn has_capacity(&self) -> bool {
        let in_flight = self.counters.records_in_flight.load(Ordering::Relaxed);
        crate::metrics::BACKFILL_SINK_QUEUED_REPOS
            .set(i64::try_from(self.queued()).unwrap_or(i64::MAX));
        crate::metrics::BACKFILL_SINK_RECORDS_IN_FLIGHT
            .set(i64::try_from(in_flight).unwrap_or(i64::MAX));
        self.sender()
            .is_some_and(|tx| tx.capacity() > 0 && in_flight < self.max_records_in_flight)
    }

    async fn ingest(&self, did: &str, body: RepoBody) -> Result<Receipt, WintermuteError> {
        let parsed = super::parse_body(did, body).await?;
        let ParsedRepo {
            did,
            rev,
            jobs,
            filtered,
            ..
        } = parsed;
        let records = jobs.len();
        let (done_tx, done_rx) = oneshot::channel();
        if jobs.is_empty() {
            // Nothing to write; the repo is done as soon as it parsed.
            drop(done_tx.send(Ok(())));
        } else {
            let closed = || WintermuteError::Other("backfill sink closed".into());
            let tx = self.sender().ok_or_else(closed)?;
            self.counters
                .records_in_flight
                .fetch_add(records, Ordering::Relaxed);
            self.counters
                .repos_in_flight
                .fetch_add(1, Ordering::Relaxed);
            if let Err(rejected) = tx
                .send(Pending {
                    did,
                    jobs,
                    done: done_tx,
                })
                .await
            {
                // Never accepted: keep the in-flight counters honest.
                self.counters
                    .records_in_flight
                    .fetch_sub(rejected.0.jobs.len(), Ordering::Relaxed);
                self.counters
                    .repos_in_flight
                    .fetch_sub(1, Ordering::Relaxed);
                return Err(closed());
            }
        }
        Ok(Receipt {
            records,
            filtered,
            rev,
            committed: done_rx,
        })
    }

    /// Close the intake and wait for every writer to flush what it holds.
    /// Writers exit once the last sender is gone; an `ingest` still holding a
    /// clone finishes its send first, then sees the sink closed.
    async fn finish(&self) {
        drop(
            self.tx
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take(),
        );
        let mut writers = self.writers.lock().await;
        for handle in writers.iter_mut() {
            drop(handle.await);
        }
        writers.clear();
    }

    fn uncommitted(&self) -> (usize, usize) {
        (
            self.counters.repos_in_flight.load(Ordering::Relaxed),
            self.counters.records_in_flight.load(Ordering::Relaxed),
        )
    }

    /// Abort the writers. Every `Pending` they hold is dropped with them, so
    /// each repo's `committed` receipt resolves as dropped and the runner's
    /// completion task returns the row to pending instead of completing it; a
    /// row whose completion task is gone too stays claimed and is recovered
    /// on restart. Nothing completes twice: a receipt resolves exactly once,
    /// whichever way.
    async fn abandon(&self) {
        drop(
            self.tx
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take(),
        );
        // `finish` holds this lock while it waits on a writer; the caller
        // drops it first. Bounded anyway, so a stop can never park here.
        let Ok(mut writers) = tokio::time::timeout(ABANDON_STEP, self.writers.lock()).await else {
            tracing::warn!("backfill sink: writers still locked by the drain; not waiting");
            return;
        };
        let handles: Vec<_> = writers.drain(..).collect();
        drop(writers);
        for handle in &handles {
            handle.abort();
        }
        let total = handles.len();
        let mut unsettled = 0usize;
        for handle in handles {
            if tokio::time::timeout(ABANDON_STEP, handle).await.is_err() {
                unsettled += 1;
            }
        }
        if unsettled > 0 {
            tracing::warn!(
                unsettled,
                total,
                "backfill sink: writers did not settle after abort (blocked in a call that \
                 cannot be cancelled); leaving them to the runtime shutdown"
            );
        }
    }
}

/// How long `abandon` waits on any one step: the writers lock, then each
/// aborted writer.
const ABANDON_STEP: Duration = Duration::from_secs(1);

/// Pull repos off the shared receiver until a batch is full (or the queue goes
/// quiet), write it, and settle every repo in it.
async fn writer_loop(
    id: usize,
    rx: Arc<Mutex<mpsc::Receiver<Pending>>>,
    pool: Pool,
    cfg: PgSinkConfig,
    counters: Arc<SinkCounters>,
) {
    loop {
        let batch = {
            let mut rx = rx.lock().await;
            let Some(first) = rx.recv().await else {
                tracing::debug!(writer = id, "backfill writer: channel closed");
                return;
            };
            let mut batch = vec![first];
            let mut jobs: usize = batch[0].jobs.len();
            let deadline = tokio::time::Instant::now() + cfg.flush_after;
            while jobs < cfg.batch_jobs {
                match tokio::time::timeout_at(deadline, rx.recv()).await {
                    Ok(Some(p)) => {
                        jobs += p.jobs.len();
                        batch.push(p);
                    }
                    Ok(None) | Err(_) => break,
                }
            }
            batch
        };

        write_batch(&pool, &cfg, &counters, batch).await;
    }
}

async fn write_batch(
    pool: &Pool,
    cfg: &PgSinkConfig,
    counters: &SinkCounters,
    batch: Vec<Pending>,
) {
    let mut jobs: Vec<(Vec<u8>, IndexJob)> = Vec::new();
    for p in &batch {
        for job in &p.jobs {
            jobs.push((job.uri.clone().into_bytes(), job.clone()));
        }
    }
    let n_jobs = jobs.len();
    let started = std::time::Instant::now();

    // bulk_load = true: skip inline aggregates and the like-insert semaphore.
    // Aggregates are recomputed after a bulk backfill, as with car_loader.
    let (results, batch_failed) =
        IndexerManager::process_jobs_batch(pool, &jobs, true, cfg.skip_boilerplate).await;
    drop(jobs);

    let failures = results.iter().filter(|(_, r)| r.is_err()).count();
    if failures > 0 {
        if let Some((_, Err(e))) = results.iter().find(|(_, r)| r.is_err()) {
            tracing::warn!(
                failures,
                total = n_jobs,
                first_error = %e,
                "backfill batch: some records failed"
            );
        }
        counters
            .record_failures
            .fetch_add(failures as u64, Ordering::Relaxed);
    }
    counters.batches.fetch_add(1, Ordering::Relaxed);
    counters
        .repos_in_flight
        .fetch_sub(batch.len(), Ordering::Relaxed);
    let remaining = counters
        .records_in_flight
        .fetch_sub(n_jobs, Ordering::Relaxed)
        .saturating_sub(n_jobs);
    crate::metrics::BACKFILL_SINK_RECORDS_IN_FLIGHT
        .set(i64::try_from(remaining).unwrap_or(i64::MAX));
    crate::metrics::BACKFILL_WRITE_SECONDS.observe(started.elapsed().as_secs_f64());

    if batch_failed {
        counters.batches_failed.fetch_add(1, Ordering::Relaxed);
        tracing::warn!(
            repos = batch.len(),
            records = n_jobs,
            "backfill batch failed; repos returned for retry"
        );
        for p in batch {
            drop(
                p.done
                    .send(Err(WintermuteError::Other("batch write failed".into()))),
            );
        }
        return;
    }

    counters
        .repos_written
        .fetch_add(batch.len() as u64, Ordering::Relaxed);
    counters
        .records_written
        .fetch_add(n_jobs as u64, Ordering::Relaxed);
    crate::metrics::BACKFILL_RECORDS_WRITTEN_TOTAL.inc_by(n_jobs as u64);
    tracing::debug!(
        repos = batch.len(),
        records = n_jobs,
        ms = started.elapsed().as_millis(),
        "backfill batch written"
    );
    for p in batch {
        tracing::trace!(did = %p.did, "repo committed");
        drop(p.done.send(Ok(())));
    }
}

/// A sink that parses and counts but writes nothing. For measuring fetch and
/// parse throughput without a database, and for enumeration-only commands.
#[derive(Default)]
pub struct NullSink {
    pub repos: AtomicU64,
    pub records: AtomicU64,
}

impl RecordSink for NullSink {
    fn has_capacity(&self) -> bool {
        true
    }

    async fn ingest(&self, did: &str, body: RepoBody) -> Result<Receipt, WintermuteError> {
        let parsed = super::parse_body(did, body).await?;
        self.repos.fetch_add(1, Ordering::Relaxed);
        self.records
            .fetch_add(parsed.jobs.len() as u64, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        drop(tx.send(Ok(())));
        Ok(Receipt {
            records: parsed.jobs.len(),
            filtered: parsed.filtered,
            rev: parsed.rev,
            committed: rx,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_null_sink_parses_and_always_has_room() {
        let sink = NullSink::default();
        assert!(sink.has_capacity());
        // Garbage is a parse error, surfaced immediately.
        let err = sink
            .ingest("did:plc:x", RepoBody::Memory(vec![0, 1, 2]))
            .await
            .unwrap_err();
        assert!(matches!(err, WintermuteError::Repo(_)));
        assert_eq!(sink.repos.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn pg_sink_defaults_are_sane() {
        let cfg = PgSinkConfig::default();
        assert!(cfg.writers >= 1);
        assert!(cfg.batch_jobs >= 100);
        assert!(cfg.queue_repos >= 1);
        assert!(cfg.max_records_in_flight >= cfg.batch_jobs);
    }

    #[tokio::test]
    async fn the_sink_reports_no_room_once_records_in_flight_reach_the_bound() {
        // A pool that never connects: nothing is written, so in-flight only grows.
        let mut pg = deadpool_postgres::Config::new();
        pg.url = Some("postgres://nobody@127.0.0.1:1/none".to_owned());
        pg.manager = Some(deadpool_postgres::ManagerConfig {
            recycling_method: deadpool_postgres::RecyclingMethod::Fast,
        });
        pg.pool = Some(crate::config::pg_pool_config(1));
        let pool = pg
            .create_pool(
                Some(deadpool_postgres::Runtime::Tokio1),
                tokio_postgres::NoTls,
            )
            .unwrap();
        let sink = PgSink::new(
            &pool,
            &PgSinkConfig {
                writers: 1,
                queue_repos: 8,
                max_records_in_flight: 5,
                ..PgSinkConfig::default()
            },
        );
        assert!(sink.has_capacity());
        sink.counters.records_in_flight.store(5, Ordering::Relaxed);
        assert!(!sink.has_capacity(), "records, not repos, are the bound");
        sink.counters.records_in_flight.store(4, Ordering::Relaxed);
        assert!(sink.has_capacity());
        assert_eq!(sink.records_in_flight(), 4);
        assert_eq!(sink.queued(), 0);
    }
}
