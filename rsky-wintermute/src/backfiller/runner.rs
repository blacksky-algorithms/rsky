//! The backfill loop: enumerate into keyed state, then drain on demand.
//!
//! The shape that matters is the gate in [`Runner::await_capacity`].
//!
//! Today the producer runs unconditionally at ingester startup, once per relay
//! host, and never consults `BACKFILLER_WORKERS`. Production runs with workers
//! at 0, so three enumerators walk 41.5 M DIDs into a queue whose consumer
//! returns immediately and sleeps forever. Bounding that queue caps the damage;
//! it does not address the cause.
//!
//! The cause is that supply and demand are three orders of magnitude apart.
//! Measured: hubble serves ~72,000 records/s at concurrency 16, and wintermute
//! writes ~521 records/s. Any open-loop producer wins that race and fills a
//! queue until something restarts it. So the fetcher here asks the *downstream*
//! queue whether there is room before claiming work, and stops pulling when
//! there is not.

use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::stream::{self, StreamExt};

use super::BackfillerManager;
use super::hubble::status_is_unfetchable;
use super::source::{
    Class, RepoBody, RepoSource, SourceError, backoff_secs, classify, cooldown_secs,
    retry_after_secs,
};
use super::state::{RepoStateStore, StateError};
use crate::SHUTDOWN;
use crate::storage::Storage;
use crate::types::WintermuteError;

/// How often the demand gate re-checks the downstream queue while paused.
const CAPACITY_POLL: Duration = Duration::from_secs(2);
/// Idle sleep when there is nothing pending to claim.
const IDLE_SLEEP: Duration = Duration::from_secs(5);

/// Where parsed records go, and how much backlog is already waiting there.
///
/// The runner depends on this rather than on `Storage` directly, so the control
/// flow -- which is the interesting part -- is testable without standing up
/// Fjall and LMDB, and so a future sink (staging tables, a COPY loader) can be
/// dropped in without touching the loop.
pub trait RecordSink: Send + Sync {
    /// Current backlog. This is what "demand" means to the gate.
    fn depth(&self) -> usize;

    /// Parse an archive and enqueue its records. Returns records enqueued.
    fn ingest(
        &self,
        did: &str,
        body: RepoBody,
        priority: bool,
    ) -> impl Future<Output = Result<usize, WintermuteError>> + Send;
}

/// The production sink: wintermute's `firehose_backfill` queue, via the
/// existing CAR parser.
pub struct StorageSink {
    storage: Arc<Storage>,
}

impl StorageSink {
    #[must_use]
    pub const fn new(storage: Arc<Storage>) -> Self {
        Self { storage }
    }
}

impl RecordSink for StorageSink {
    fn depth(&self) -> usize {
        self.storage.firehose_backfill_len().unwrap_or(0)
    }

    async fn ingest(
        &self,
        did: &str,
        body: RepoBody,
        priority: bool,
    ) -> Result<usize, WintermuteError> {
        // The existing parser, unchanged. Verified against real hubble
        // archives: standard v3 commit, complete MST, every block hashing to
        // its CID -- so there is no hubble-specific parse path to write.
        match body {
            RepoBody::Memory(bytes) => {
                BackfillerManager::process_car_reader(
                    &self.storage,
                    did,
                    Cursor::new(&bytes[..]),
                    priority,
                )
                .await
            }
            RepoBody::Spilled { file, .. } => {
                let handle = file
                    .reopen()
                    .map_err(|e| WintermuteError::Other(format!("spill reopen: {e}")))?;
                BackfillerManager::process_car_reader(
                    &self.storage,
                    did,
                    tokio::fs::File::from_std(handle),
                    priority,
                )
                .await
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunnerConfig {
    /// Concurrent in-flight fetches. Measured: throughput is linear to 16 and
    /// flat past it, so anything above ~8 buys latency and nothing else -- and
    /// we need a fraction of even that to outrun the writer.
    pub concurrency: usize,
    /// Repos claimed from state per round.
    pub claim_batch: usize,
    /// Stop claiming when the sink's backlog reaches this depth.
    pub queue_high_watermark: usize,
    /// Resume once it falls back to this. Hysteresis, so the gate does not flap
    /// at the boundary.
    pub queue_low_watermark: usize,
    pub page_limit: u32,
    /// Enqueue parsed records ahead of normal backfill traffic.
    pub priority: bool,
    pub spill_threshold: u64,
    pub max_body: u64,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            concurrency: 4,
            claim_batch: 32,
            queue_high_watermark: 200_000,
            queue_low_watermark: 50_000,
            page_limit: 1000,
            priority: false,
            spill_threshold: 1 << 20,
            max_body: 512 << 20,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EnumerateReport {
    pub pages: u64,
    pub seen: u64,
    pub needs_fetch: u64,
    pub unchanged: u64,
    pub skipped: u64,
    /// True when the source ran out of cursor, i.e. a full pass completed.
    pub completed: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DrainReport {
    pub attempted: u64,
    pub repos_indexed: u64,
    pub records_enqueued: u64,
    pub failed_transient: u64,
    pub failed_terminal: u64,
    pub spilled: u64,
    /// How many times the demand gate held us back. Non-zero here is the system
    /// working, not a problem.
    pub throttle_waits: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum RunnerError {
    #[error("source: {0}")]
    Source(#[from] SourceError),
    #[error("state: {0}")]
    State(#[from] StateError),
    #[error("wintermute: {0}")]
    Wintermute(#[from] WintermuteError),
}

/// One repo's outcome. Kept separate from [`RunnerError`] because a per-repo
/// failure is something to record, not a reason to stop the loop.
enum JobOutcome {
    Indexed {
        records: usize,
        spilled: bool,
    },
    Failed {
        class: Class,
        retry_after: Option<u64>,
        detail: String,
    },
}

pub struct Runner<S, K> {
    source: S,
    sink: K,
    state: RepoStateStore,
    cfg: RunnerConfig,
    throttled: AtomicBool,
}

impl<S: RepoSource, K: RecordSink> Runner<S, K> {
    pub const fn new(source: S, sink: K, state: RepoStateStore, cfg: RunnerConfig) -> Self {
        Self {
            source,
            sink,
            state,
            cfg,
            throttled: AtomicBool::new(false),
        }
    }

    fn cursor_key(&self) -> String {
        format!("backfill_enum:{}", self.source.name())
    }

    /// Walk the source's `listRepos`, recording every repo in keyed state.
    ///
    /// Resumable and idempotent. The cursor is persisted as text after each
    /// page, so a restart continues where it stopped -- including for hubble,
    /// whose cursor is a DID and which the existing producer would truncate to
    /// `0`, re-walking from the start of the keyspace every time.
    pub async fn enumerate(&self) -> Result<EnumerateReport, RunnerError> {
        self.enumerate_pages(None).await
    }

    /// [`Self::enumerate`], stopping after `max_pages`. The cursor is persisted
    /// either way, so a capped run is just a shorter resumable pass -- which is
    /// what makes it safe to try against the live service.
    pub async fn enumerate_pages(
        &self,
        max_pages: Option<u64>,
    ) -> Result<EnumerateReport, RunnerError> {
        let key = self.cursor_key();
        let mut cursor = self.state.get_cursor(&key)?;
        let mut report = EnumerateReport::default();

        if let Some(ref c) = cursor {
            tracing::info!(source = self.source.name(), cursor = %c, "enumeration resuming");
        }

        loop {
            if SHUTDOWN.load(Ordering::Relaxed) {
                tracing::info!("enumeration: shutdown requested");
                return Ok(report);
            }

            let page = self
                .source
                .list_repos(cursor.as_deref(), self.cfg.page_limit)
                .await?;

            report.pages += 1;
            report.seen += page.repos.len() as u64;

            let (fetch, unchanged, skipped) = self.state.upsert_batch(
                &page.repos,
                self.source.name(),
                // Protocol-level, not source-specific: an account upstream
                // reports as deleted or taken down will 400 on getRepo, so it
                // is written off at enumeration rather than one failed fetch at
                // a time.
                &|r| status_is_unfetchable(r.active, r.status.as_deref()),
            )?;
            report.needs_fetch += fetch;
            report.unchanged += unchanged;
            report.skipped += skipped;

            let Some(next) = page.cursor else {
                // A completed pass clears the cursor, so the next pass starts
                // fresh instead of resuming from end-of-keyspace.
                self.state.clear_cursor(&key)?;
                report.completed = true;
                tracing::info!(?report, "enumeration pass complete");
                return Ok(report);
            };

            // Guard against a source that hands back the cursor we just sent:
            // that is a page loop, not progress.
            if cursor.as_deref() == Some(next.as_str()) {
                tracing::warn!(
                    source = self.source.name(),
                    cursor = %next,
                    "enumeration: source returned the cursor we sent; stopping"
                );
                return Ok(report);
            }
            self.state.set_cursor(&key, &next)?;
            cursor = Some(next);

            if max_pages.is_some_and(|max| report.pages >= max) {
                tracing::info!(?report, "enumeration: page cap reached");
                return Ok(report);
            }
            if report.pages % 25 == 0 {
                tracing::info!(
                    pages = report.pages,
                    seen = report.seen,
                    needs_fetch = report.needs_fetch,
                    unchanged = report.unchanged,
                    "enumeration progress"
                );
            }
        }
    }

    /// Block until the sink has drained enough to justify more fetching.
    ///
    /// Returns `false` if shutdown was requested while waiting.
    async fn await_capacity(&self, report: &mut DrainReport) -> bool {
        loop {
            if SHUTDOWN.load(Ordering::Relaxed) {
                return false;
            }
            let depth = self.sink.depth();

            if self.throttled.load(Ordering::Relaxed) {
                if depth <= self.cfg.queue_low_watermark {
                    self.throttled.store(false, Ordering::Relaxed);
                    tracing::info!(depth, "backfill resuming: indexer has caught up");
                    return true;
                }
            } else if depth < self.cfg.queue_high_watermark {
                return true;
            } else {
                self.throttled.store(true, Ordering::Relaxed);
                report.throttle_waits += 1;
                tracing::info!(
                    depth,
                    high = self.cfg.queue_high_watermark,
                    low = self.cfg.queue_low_watermark,
                    "backfill pausing: downstream queue at high watermark"
                );
            }
            tokio::time::sleep(CAPACITY_POLL).await;
        }
    }

    /// Fetch one repo and hand its records to the sink.
    async fn run_job(&self, did: &str) -> JobOutcome {
        let body = match self
            .source
            .fetch_repo(did, self.cfg.spill_threshold, self.cfg.max_body)
            .await
        {
            Ok(b) => b,
            Err(e) => {
                return JobOutcome::Failed {
                    class: classify(&e),
                    retry_after: retry_after_secs(&e),
                    detail: e.to_string(),
                };
            }
        };

        let spilled = body.spilled();
        match self.sink.ingest(did, body, self.cfg.priority).await {
            Ok(records) => JobOutcome::Indexed { records, spilled },
            // A CAR we cannot parse will not parse next time either.
            Err(e) => JobOutcome::Failed {
                class: Class::Terminal,
                retry_after: None,
                detail: format!("ingest: {e}"),
            },
        }
    }

    /// One round: gate on demand, claim a batch, fetch it concurrently, record
    /// every outcome. Returns how many repos were attempted; zero means there
    /// was nothing to do.
    pub async fn drain_round(&self, report: &mut DrainReport) -> Result<usize, RunnerError> {
        if !self.await_capacity(report).await {
            return Ok(0);
        }

        let claimed = self.state.claim(self.cfg.claim_batch)?;
        if claimed.is_empty() {
            return Ok(0);
        }

        let outcomes = stream::iter(claimed)
            .map(|(did, rev)| async move {
                let outcome = self.run_job(&did).await;
                (did, rev, outcome)
            })
            .buffer_unordered(self.cfg.concurrency)
            .collect::<Vec<_>>()
            .await;

        let attempted = outcomes.len();
        for (did, rev, outcome) in outcomes {
            report.attempted += 1;
            match outcome {
                JobOutcome::Indexed { records, spilled } => {
                    self.state.complete(&did, &rev)?;
                    report.repos_indexed += 1;
                    report.records_enqueued += records as u64;
                    if spilled {
                        report.spilled += 1;
                    }
                    tracing::debug!(did = %did, records, spilled, "backfilled");
                }
                JobOutcome::Failed {
                    class,
                    retry_after,
                    detail,
                } => {
                    let transient = class.is_transient();
                    let cooldown = if class == Class::RateLimited {
                        cooldown_secs(retry_after)
                    } else {
                        backoff_secs(1)
                    };
                    let next = self.state.fail(&did, transient, cooldown, &detail)?;
                    if transient {
                        report.failed_transient += 1;
                    } else {
                        report.failed_terminal += 1;
                    }
                    tracing::warn!(
                        did = %did,
                        class = class.as_str(),
                        next = next.as_str(),
                        cooldown_secs = cooldown,
                        detail = %detail,
                        "backfill job failed"
                    );
                }
            }
        }
        Ok(attempted)
    }

    /// Drain until `max_repos` have been attempted, or forever when `None`.
    /// Recovers rows left claimed by a previous process first.
    pub async fn drain(&self, max_repos: Option<u64>) -> Result<DrainReport, RunnerError> {
        let recovered = self.state.reset_claimed()?;
        if recovered > 0 {
            tracing::info!(recovered, "returned claimed rows to pending after restart");
        }

        let mut report = DrainReport::default();
        loop {
            if SHUTDOWN.load(Ordering::Relaxed) {
                tracing::info!(?report, "drain: shutdown requested");
                return Ok(report);
            }
            if max_repos.is_some_and(|max| report.attempted >= max) {
                return Ok(report);
            }

            let n = self.drain_round(&mut report).await?;
            if n == 0 && !SHUTDOWN.load(Ordering::Relaxed) {
                tokio::time::sleep(IDLE_SLEEP).await;
            }
        }
    }

    #[must_use]
    pub const fn config(&self) -> &RunnerConfig {
        &self.cfg
    }

    #[must_use]
    pub const fn repo_state(&self) -> &RepoStateStore {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backfiller::source::{RepoPage, RepoRef};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    /// Serves canned pages and canned fetch results, so the runner's control
    /// flow is testable without a network.
    struct FakeSource {
        pages: Mutex<Vec<RepoPage>>,
        fetches: Mutex<Vec<Result<Vec<u8>, SourceError>>>,
    }

    impl FakeSource {
        fn new(pages: Vec<RepoPage>, fetches: Vec<Result<Vec<u8>, SourceError>>) -> Self {
            Self {
                pages: Mutex::new(pages),
                fetches: Mutex::new(fetches),
            }
        }
        fn pages(pages: Vec<RepoPage>) -> Self {
            Self::new(pages, vec![])
        }
    }

    impl RepoSource for FakeSource {
        fn name(&self) -> &'static str {
            "fake"
        }

        async fn list_repos(
            &self,
            _cursor: Option<&str>,
            _limit: u32,
        ) -> Result<RepoPage, SourceError> {
            let mut pages = self.pages.lock().unwrap();
            if pages.is_empty() {
                return Ok(RepoPage::default());
            }
            Ok(pages.remove(0))
        }

        async fn fetch_repo(
            &self,
            _did: &str,
            _spill: u64,
            _cap: u64,
        ) -> Result<RepoBody, SourceError> {
            let mut f = self.fetches.lock().unwrap();
            if f.is_empty() {
                return Err(SourceError::Transport("no canned fetch".into()));
            }
            f.remove(0).map(RepoBody::Memory)
        }
    }

    /// Counts records and reports a caller-controlled backlog depth.
    struct FakeSink {
        depth: AtomicUsize,
        ingested: AtomicUsize,
        /// Records reported per archive, so a test can pretend a repo was big.
        records_per_repo: usize,
    }

    impl FakeSink {
        fn new(depth: usize) -> Self {
            Self {
                depth: AtomicUsize::new(depth),
                ingested: AtomicUsize::new(0),
                records_per_repo: 10,
            }
        }
    }

    impl RecordSink for FakeSink {
        fn depth(&self) -> usize {
            self.depth.load(AtomicOrdering::Relaxed)
        }
        async fn ingest(
            &self,
            _did: &str,
            _body: RepoBody,
            _priority: bool,
        ) -> Result<usize, WintermuteError> {
            self.ingested.fetch_add(1, AtomicOrdering::Relaxed);
            Ok(self.records_per_repo)
        }
    }

    fn repo(did: &str, rev: &str) -> RepoRef {
        RepoRef {
            did: did.into(),
            rev: rev.into(),
            active: true,
            status: None,
        }
    }

    fn cfg() -> RunnerConfig {
        RunnerConfig {
            concurrency: 2,
            claim_batch: 8,
            ..RunnerConfig::default()
        }
    }

    #[tokio::test]
    async fn enumeration_walks_pages_and_stops_at_the_end() {
        let pages = vec![
            RepoPage {
                repos: vec![repo("did:a", "r1"), repo("did:b", "r1")],
                cursor: Some("did:b".into()),
            },
            RepoPage {
                repos: vec![repo("did:c", "r1")],
                cursor: None,
            },
        ];
        let state = RepoStateStore::open_in_memory().unwrap();
        let runner = Runner::new(
            FakeSource::pages(pages),
            FakeSink::new(0),
            state.clone(),
            cfg(),
        );
        let r = runner.enumerate().await.unwrap();
        assert_eq!((r.pages, r.seen, r.needs_fetch), (2, 3, 3));
        assert!(r.completed);
        assert_eq!(
            state.get_cursor("backfill_enum:fake").unwrap(),
            None,
            "a completed pass clears the cursor"
        );
    }

    #[tokio::test]
    async fn enumeration_persists_a_did_cursor_and_resumes_from_it() {
        // The regression this replaces: the existing producer would have
        // written 0 here and re-walked from the start.
        let pages = vec![RepoPage {
            repos: vec![repo("did:a", "r1")],
            cursor: Some("did:plc:222rpxpbdd4cd2opywsqpxtz".into()),
        }];
        let state = RepoStateStore::open_in_memory().unwrap();
        let runner = Runner::new(
            FakeSource::pages(pages),
            FakeSink::new(0),
            state.clone(),
            cfg(),
        );
        // Cap at one page so the pass is interrupted rather than completed:
        // a completed pass deliberately clears the cursor.
        let r = runner.enumerate_pages(Some(1)).await.unwrap();
        assert!(!r.completed, "stopped at the page cap, mid-pass");
        assert_eq!(
            state.get_cursor("backfill_enum:fake").unwrap().as_deref(),
            Some("did:plc:222rpxpbdd4cd2opywsqpxtz"),
            "a DID cursor survives verbatim; the i64 column it replaces stores 0"
        );
    }

    #[tokio::test]
    async fn a_repeated_cursor_is_treated_as_a_loop() {
        let state = RepoStateStore::open_in_memory().unwrap();
        state.set_cursor("backfill_enum:fake", "stuck").unwrap();
        let pages = vec![RepoPage {
            repos: vec![repo("did:a", "r1")],
            cursor: Some("stuck".into()),
        }];
        let runner = Runner::new(FakeSource::pages(pages), FakeSink::new(0), state, cfg());
        let r = runner.enumerate().await.unwrap();
        assert_eq!(r.pages, 1);
        assert!(!r.completed, "a loop is not a completed pass");
    }

    #[tokio::test]
    async fn a_second_pass_over_indexed_repos_creates_no_work() {
        let make = || {
            vec![RepoPage {
                repos: vec![repo("did:a", "r1"), repo("did:b", "r1")],
                cursor: None,
            }]
        };
        let state = RepoStateStore::open_in_memory().unwrap();
        let runner = Runner::new(
            FakeSource::pages(make()),
            FakeSink::new(0),
            state.clone(),
            cfg(),
        );
        assert_eq!(runner.enumerate().await.unwrap().needs_fetch, 2);
        for (did, rev) in state.claim(10).unwrap() {
            state.complete(&did, &rev).unwrap();
        }

        let runner = Runner::new(
            FakeSource::pages(make()),
            FakeSink::new(0),
            state.clone(),
            cfg(),
        );
        let r = runner.enumerate().await.unwrap();
        assert_eq!(
            (r.needs_fetch, r.unchanged),
            (0, 2),
            "the property the timestamp-keyed queue cannot have"
        );
        assert!(state.claim(10).unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_round_fetches_claimed_repos_and_marks_them_done() {
        let state = RepoStateStore::open_in_memory().unwrap();
        state.upsert(&repo("did:a", "r1"), "fake", false).unwrap();
        state.upsert(&repo("did:b", "r1"), "fake", false).unwrap();
        let source = FakeSource::new(vec![], vec![Ok(vec![1, 2, 3]), Ok(vec![4, 5, 6])]);
        let runner = Runner::new(source, FakeSink::new(0), state.clone(), cfg());

        let mut report = DrainReport::default();
        let n = runner.drain_round(&mut report).await.unwrap();
        assert_eq!(n, 2);
        assert_eq!((report.repos_indexed, report.records_enqueued), (2, 20));
        let st = state.stats().unwrap();
        assert_eq!((st.done, st.pending), (2, 0));
    }

    #[tokio::test]
    async fn the_demand_gate_refuses_to_claim_when_the_sink_is_full() {
        // The whole point. With the backlog above the high watermark, a round
        // must claim nothing at all -- not claim and buffer.
        let state = RepoStateStore::open_in_memory().unwrap();
        state.upsert(&repo("did:a", "r1"), "fake", false).unwrap();
        let source = FakeSource::new(vec![], vec![Ok(vec![1])]);
        let c = RunnerConfig {
            queue_high_watermark: 100,
            queue_low_watermark: 10,
            ..cfg()
        };
        let sink = FakeSink::new(500);
        let runner = Runner::new(source, sink, state.clone(), c);

        let mut report = DrainReport::default();
        // Gate parks the round; SHUTDOWN releases it so the test terminates.
        let handle = tokio::spawn(async {
            tokio::time::sleep(Duration::from_millis(60)).await;
            SHUTDOWN.store(true, Ordering::Relaxed);
        });
        let n = runner.drain_round(&mut report).await.unwrap();
        handle.await.unwrap();
        SHUTDOWN.store(false, Ordering::Relaxed);

        assert_eq!(n, 0, "nothing claimed while the sink is over its watermark");
        assert!(report.throttle_waits >= 1, "and it said so");
        assert_eq!(
            state.stats().unwrap().pending,
            1,
            "the repo is still pending, not claimed and stranded"
        );
    }

    #[tokio::test]
    async fn a_transient_fetch_failure_returns_the_repo_to_pending() {
        let state = RepoStateStore::open_in_memory().unwrap();
        state.upsert(&repo("did:a", "r1"), "fake", false).unwrap();
        let source = FakeSource::new(
            vec![],
            vec![Err(SourceError::Status {
                status: 503,
                retry_after_secs: None,
            })],
        );
        let runner = Runner::new(source, FakeSink::new(0), state.clone(), cfg());

        let mut report = DrainReport::default();
        runner.drain_round(&mut report).await.unwrap();
        assert_eq!(report.failed_transient, 1);
        assert_eq!(report.failed_terminal, 0);
        assert_eq!(state.stats().unwrap().pending, 1);
    }

    #[tokio::test]
    async fn a_terminal_fetch_failure_writes_the_repo_off_immediately() {
        let state = RepoStateStore::open_in_memory().unwrap();
        state.upsert(&repo("did:a", "r1"), "fake", false).unwrap();
        let source = FakeSource::new(
            vec![],
            vec![Err(SourceError::Status {
                status: 404,
                retry_after_secs: None,
            })],
        );
        let runner = Runner::new(source, FakeSink::new(0), state.clone(), cfg());

        let mut report = DrainReport::default();
        runner.drain_round(&mut report).await.unwrap();
        assert_eq!(report.failed_terminal, 1);
        assert_eq!(state.stats().unwrap().terminal, 1);
    }

    #[tokio::test]
    async fn a_rate_limit_parks_the_repo_for_the_advertised_interval() {
        let state = RepoStateStore::open_in_memory().unwrap();
        state.upsert(&repo("did:a", "r1"), "fake", false).unwrap();
        let source = FakeSource::new(
            vec![],
            vec![Err(SourceError::Status {
                status: 429,
                retry_after_secs: Some(120),
            })],
        );
        let runner = Runner::new(source, FakeSink::new(0), state.clone(), cfg());

        let mut report = DrainReport::default();
        runner.drain_round(&mut report).await.unwrap();
        assert_eq!(report.failed_transient, 1);
        // Pending, but cooling down, so a follow-up round finds nothing.
        assert_eq!(state.stats().unwrap().pending, 1);
        assert!(state.claim(10).unwrap().is_empty(), "still cooling down");
    }

    #[tokio::test]
    async fn an_empty_round_is_not_an_error() {
        let state = RepoStateStore::open_in_memory().unwrap();
        let runner = Runner::new(FakeSource::pages(vec![]), FakeSink::new(0), state, cfg());
        let mut report = DrainReport::default();
        assert_eq!(runner.drain_round(&mut report).await.unwrap(), 0);
        assert_eq!(report.attempted, 0);
    }
}
