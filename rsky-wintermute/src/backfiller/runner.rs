//! The backfill loop: discover hosts, enumerate into keyed state, then drain
//! per source, on demand.
//!
//! Two shapes matter here.
//!
//! **The demand gate.** The old producer ran unconditionally and filled a queue
//! that nothing drained; measured supply (hubble at the knee, ~72,000
//! records/s) and demand (Postgres, 7,000-9,300 records/s on 8 vCPU) are an
//! order of magnitude apart, so any open-loop fetcher wins the race to OOM.
//! Workers here ask the sink for room before claiming a single repo.
//!
//! **One worker per source.** Every PDS host gets its own worker with its own
//! rate budget and adaptive concurrency, as does hubble. Bluesky's ~90
//! mushrooms therefore fetch in parallel under ~90 independent budgets instead
//! of one shared byte-capped pipe, and a struggling small PDS only slows
//! itself down.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::task::JoinSet;

use super::discovery::{DiscoveredHost, list_hosts};
use super::host::{HostHealth, HostPolicy, Hostname};
use super::hubble::{HubbleConfig, HubbleSource};
use super::pds::PdsSource;
use super::sink::{Receipt, RecordSink};
use super::source::{
    Class, FetchLimits, RepoSource, SourceError, backoff_secs, classify, cooldown_secs,
    retry_after_secs, sleep_secs, status_is_unfetchable,
};
use super::state::{HUBBLE_SOURCE, HostRow, ListState, RepoStateStore, StateError};
use crate::SHUTDOWN;
use crate::metrics;
use crate::types::WintermuteError;

/// Cooldown applied when a host's `listRepos` stopped short but is worth
/// resuming: the coordinator should not immediately re-claim and hammer it.
const LIST_RETRY_COOLDOWN_SECS: u64 = 300;

#[derive(Debug, Clone)]
pub struct RunnerConfig {
    /// hubble, if it is a source at all.
    pub hubble: Option<HubbleConfig>,
    /// Whether hosts matching the policy's direct patterns are fetched from
    /// directly.
    pub direct: bool,
    /// Relay for `listHosts`, with scheme. `None` means no discovery: only
    /// `extra_hosts` are direct.
    pub relay_url: Option<String>,
    pub extra_hosts: Vec<Hostname>,
    pub policy: HostPolicy,
    /// In-flight fetches against hubble.
    pub hubble_concurrency: usize,
    /// Concurrent per-source workers.
    pub max_workers: usize,
    /// Repos claimed per round, as a multiple of the source's concurrency.
    pub claim_multiplier: usize,
    pub page_limit: u32,
    pub fetch: FetchLimits,
    pub reenumerate_after: Option<Duration>,
    pub user_agent: String,
    /// How often a gated worker re-checks the sink.
    pub gate_poll: Duration,
    /// A worker with nothing to claim for this long exits; the coordinator
    /// restarts it when work appears.
    pub idle_exit: Duration,
    pub coordinator_poll: Duration,
    /// Retries for one enumeration page. Losing a page silently skips repos.
    pub list_max_retries: u32,
    pub connect_timeout: Duration,
    /// How often progress is logged and state gauges sampled.
    pub progress_every: Duration,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            hubble: Some(HubbleConfig::default()),
            direct: true,
            relay_url: None,
            extra_hosts: Vec::new(),
            policy: HostPolicy::default(),
            hubble_concurrency: 4,
            max_workers: 128,
            claim_multiplier: 2,
            page_limit: 1000,
            fetch: FetchLimits::default(),
            reenumerate_after: None,
            user_agent: super::hubble::DEFAULT_USER_AGENT.to_owned(),
            gate_poll: Duration::from_millis(250),
            idle_exit: Duration::from_secs(30),
            coordinator_poll: Duration::from_secs(2),
            list_max_retries: 10,
            connect_timeout: Duration::from_secs(15),
            progress_every: Duration::from_secs(30),
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
    pub deferred: u64,
    /// True when the source ran out of cursor, i.e. a full pass completed.
    pub completed: bool,
}

#[derive(Debug, Default)]
pub struct Progress {
    pub repos_fetched: AtomicU64,
    pub repos_done: AtomicU64,
    pub repos_failed_transient: AtomicU64,
    pub repos_failed_terminal: AtomicU64,
    pub records: AtomicU64,
    pub bytes: AtomicU64,
    pub spilled: AtomicU64,
    pub gate_waits: AtomicU64,
    pub enumeration_done: AtomicBool,
}

#[derive(Debug, thiserror::Error)]
pub enum RunnerError {
    #[error("source: {0}")]
    Source(#[from] SourceError),
    #[error("state: {0}")]
    State(#[from] StateError),
    #[error("wintermute: {0}")]
    Wintermute(#[from] WintermuteError),
    #[error("http client: {0}")]
    Client(String),
    #[error("task panicked: {0}")]
    Join(#[from] tokio::task::JoinError),
}

/// One repo's outcome. A per-repo failure is something to record, not a reason
/// to stop the loop.
enum JobOutcome {
    Accepted {
        receipt: Receipt,
        bytes: u64,
        spilled: bool,
    },
    Failed {
        class: Class,
        retry_after: Option<u64>,
        detail: String,
    },
}

/// Metric label for a source.
fn source_class(name: &str) -> &'static str {
    if name == HUBBLE_SOURCE {
        "hubble"
    } else if name.ends_with(".host.bsky.network") {
        "bsky"
    } else {
        "pds"
    }
}

pub struct Runner<K: RecordSink> {
    cfg: RunnerConfig,
    sink: Arc<K>,
    state: RepoStateStore,
    client: reqwest::Client,
    hubble: Option<Arc<HubbleSource>>,
    pub progress: Arc<Progress>,
}

impl<K: RecordSink> Runner<K> {
    pub fn new(
        cfg: RunnerConfig,
        sink: Arc<K>,
        state: RepoStateStore,
    ) -> Result<Self, RunnerError> {
        let client = reqwest::Client::builder()
            .user_agent(&cfg.user_agent)
            .connect_timeout(cfg.connect_timeout)
            .pool_max_idle_per_host(4)
            .pool_idle_timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| RunnerError::Client(e.to_string()))?;
        let hubble = cfg
            .hubble
            .clone()
            .map(|h| Arc::new(HubbleSource::with_client(h, client.clone())));
        Ok(Self {
            cfg,
            sink,
            state,
            client,
            hubble,
            progress: Arc::new(Progress::default()),
        })
    }

    #[must_use]
    pub const fn config(&self) -> &RunnerConfig {
        &self.cfg
    }

    #[must_use]
    pub const fn state(&self) -> &RepoStateStore {
        &self.state
    }

    #[must_use]
    pub const fn hubble(&self) -> Option<&Arc<HubbleSource>> {
        self.hubble.as_ref()
    }

    /// Build the direct source for a host, at the policy's budget.
    #[must_use]
    pub fn pds_source(&self, host: &Hostname) -> PdsSource {
        PdsSource::new(
            host.clone(),
            self.client.clone(),
            &self.cfg.policy.limits_for(host),
        )
    }

    // ------------------------------------------------------------ discovery

    /// Record the hosts we will fetch from directly. Returns how many.
    pub async fn discover(&self) -> Result<usize, RunnerError> {
        if !self.cfg.direct {
            return Ok(0);
        }
        let mut direct = 0usize;
        if let Some(relay) = &self.cfg.relay_url {
            let hosts = list_hosts(&self.client, relay).await?;
            direct += self.record_hosts(&hosts)?;
        }
        for host in &self.cfg.extra_hosts {
            self.state.upsert_host(host, 0, None, true)?;
            direct += 1;
        }
        tracing::info!(direct, "host discovery complete");
        Ok(direct)
    }

    fn record_hosts(&self, hosts: &[DiscoveredHost]) -> Result<usize, RunnerError> {
        let mut direct = 0usize;
        for h in hosts {
            let is_direct = self.cfg.policy.is_direct(&h.host) && h.crawlable();
            self.state
                .upsert_host(&h.host, h.account_count, h.status.as_deref(), is_direct)?;
            if is_direct {
                direct += 1;
            }
        }
        Ok(direct)
    }

    // ---------------------------------------------------------- enumeration

    /// Walk a source's `listRepos` into keyed state. Resumable: the cursor is
    /// persisted as text after each page, and a completed pass clears it.
    pub async fn enumerate_pages<S: RepoSource>(
        &self,
        source: &S,
        max_pages: Option<u64>,
    ) -> Result<EnumerateReport, RunnerError> {
        let key = format!("enum:{}", source.name());
        let mut cursor = self.state.get_cursor(&key)?;
        let mut report = EnumerateReport::default();
        let name = source.name().to_owned();

        if let Some(c) = &cursor {
            tracing::info!(source = %name, cursor = %c, "enumeration resuming");
        }

        loop {
            if SHUTDOWN.load(Ordering::Relaxed) {
                return Ok(report);
            }

            let page = self.list_page_with_retries(source, cursor.clone()).await?;
            report.pages += 1;
            report.seen += page.repos.len() as u64;

            let (fetch, unchanged, skipped, deferred) =
                self.state.upsert_batch(&page.repos, &name, &|r| {
                    status_is_unfetchable(r.active, r.status.as_deref())
                })?;
            report.needs_fetch += fetch;
            report.unchanged += unchanged;
            report.skipped += skipped;
            report.deferred += deferred;
            metrics::BACKFILL_REPOS_TERMINAL_TOTAL.inc_by(skipped);

            let Some(next) = page.cursor else {
                self.state.clear_cursor(&key)?;
                report.completed = true;
                tracing::info!(source = %name, ?report, "enumeration pass complete");
                return Ok(report);
            };
            if cursor.as_deref() == Some(next.as_str()) {
                tracing::warn!(source = %name, cursor = %next, "source returned the cursor we sent; stopping");
                self.state.clear_cursor(&key)?;
                report.completed = true;
                return Ok(report);
            }
            self.state.set_cursor(&key, &next)?;
            cursor = Some(next);

            if max_pages.is_some_and(|max| report.pages >= max) {
                return Ok(report);
            }
            if report.pages % 50 == 0 {
                tracing::info!(
                    source = %name,
                    pages = report.pages,
                    seen = report.seen,
                    needs_fetch = report.needs_fetch,
                    unchanged = report.unchanged,
                    deferred = report.deferred,
                    "enumeration progress"
                );
            }
        }
    }

    async fn list_page_with_retries<S: RepoSource>(
        &self,
        source: &S,
        cursor: Option<String>,
    ) -> Result<super::source::RepoPage, SourceError> {
        let mut attempt = 0u32;
        loop {
            match source.list_repos(cursor.clone(), self.cfg.page_limit).await {
                Ok(page) => return Ok(page),
                Err(e) if attempt < self.cfg.list_max_retries && classify(&e).is_transient() => {
                    attempt += 1;
                    let delay = retry_after_secs(&e).unwrap_or_else(|| backoff_secs(attempt));
                    tracing::warn!(
                        source = source.name(),
                        attempt,
                        delay_secs = delay,
                        error = %e,
                        "listRepos: transient failure, backing off"
                    );
                    sleep_secs(delay).await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Enumerate one direct host and record where it got to.
    pub async fn enumerate_host(&self, row: &HostRow) -> Result<(), RunnerError> {
        if row.list_state != ListState::Pending
            || row.cooldown_until > chrono::Utc::now().timestamp()
        {
            return Ok(());
        }
        let source = self.pds_source(&row.host);
        match self.enumerate_pages(&source, None).await {
            Ok(report) if report.completed => {
                self.state.set_list_state(&row.host, ListState::Done)?;
            }
            Ok(_) => {
                // Shutdown mid-pass; cursor is persisted.
            }
            Err(RunnerError::Source(e)) if classify(&e).is_transient() => {
                tracing::warn!(host = %row.host, error = %e, "listRepos incomplete; cooling down to resume later");
                self.state.set_host_cooldown(
                    &row.host,
                    LIST_RETRY_COOLDOWN_SECS,
                    &e.to_string(),
                )?;
            }
            Err(RunnerError::Source(e)) => {
                tracing::warn!(host = %row.host, error = %e, "host cannot be enumerated; marking unreachable");
                self.state
                    .set_list_state(&row.host, ListState::Unreachable)?;
            }
            Err(e) => return Err(e),
        }
        Ok(())
    }

    /// Enumerate every pending direct host, largest first, `max_workers` at a
    /// time.
    pub async fn enumerate_direct(self: &Arc<Self>) -> Result<(), RunnerError> {
        if !self.cfg.direct {
            return Ok(());
        }
        let hosts: Vec<HostRow> = self
            .state
            .direct_hosts()?
            .into_iter()
            .filter(|h| h.list_state == ListState::Pending)
            .collect();
        tracing::info!(hosts = hosts.len(), "enumerating direct hosts");
        let mut tasks: JoinSet<Result<(), RunnerError>> = JoinSet::new();
        for row in hosts {
            while tasks.len() >= self.cfg.max_workers.max(1) {
                if let Some(joined) = tasks.join_next().await {
                    joined??;
                }
            }
            let this = Arc::clone(self);
            tasks.spawn(async move { this.enumerate_host(&row).await });
        }
        while let Some(joined) = tasks.join_next().await {
            joined??;
        }
        Ok(())
    }

    /// Enumerate hubble, if it is a source.
    pub async fn enumerate_hubble(&self) -> Result<Option<EnumerateReport>, RunnerError> {
        let Some(hubble) = &self.hubble else {
            return Ok(None);
        };
        Ok(Some(self.enumerate_pages(hubble.as_ref(), None).await?))
    }

    /// Discovery, then direct hosts, then hubble. Direct first so that hubble's
    /// whole-network listing defers to the mushrooms rather than racing them.
    pub async fn enumerate_all(self: &Arc<Self>) -> Result<(), RunnerError> {
        self.discover().await?;
        self.enumerate_direct().await?;
        self.enumerate_hubble().await?;
        self.progress
            .enumeration_done
            .store(true, Ordering::Relaxed);
        metrics::BACKFILL_ENUMERATION_COMPLETE.set(1);
        Ok(())
    }

    // --------------------------------------------------------------- fetch

    async fn fetch_one<S: RepoSource>(&self, source: &S, did: &str) -> JobOutcome {
        let body = match source
            .fetch_repo(did.to_owned(), self.cfg.fetch.clone())
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
        let (bytes, spilled) = (body.len(), body.spilled());
        match self.sink.ingest(did, body).await {
            Ok(receipt) => JobOutcome::Accepted {
                receipt,
                bytes,
                spilled,
            },
            // A CAR we cannot parse will not parse next time either.
            Err(e) => JobOutcome::Failed {
                class: Class::Terminal,
                retry_after: None,
                detail: format!("parse: {e}"),
            },
        }
    }

    /// The fallback source for a repo `source` will not serve: hubble, unless
    /// this already is hubble or hubble is not configured.
    fn fallback_for(&self, source: &str) -> Option<&'static str> {
        (self.hubble.is_some() && source != HUBBLE_SOURCE).then_some(HUBBLE_SOURCE)
    }

    /// Record a fetch failure against the repo and the host.
    fn settle_failure(
        &self,
        source: &str,
        health: &HostHealth,
        did: &str,
        class: Class,
        retry_after: Option<u64>,
        detail: &str,
    ) -> Result<(), RunnerError> {
        metrics::BACKFILL_FETCH_FAILURES_TOTAL
            .with_label_values(&[source_class(source), class.as_str()])
            .inc();
        if let Some(new_limit) = health.record(Some(class), retry_after) {
            metrics::BACKFILL_HOST_REDUCTIONS_TOTAL.inc();
            tracing::warn!(
                source,
                new_limit,
                "reduced fetch concurrency after consecutive transient errors"
            );
            if source != HUBBLE_SOURCE {
                if let Ok(host) = Hostname::new(source) {
                    self.state.set_host_concurrency(&host, new_limit)?;
                }
            }
        }
        let transient = class.is_transient();
        let cooldown = if class == Class::RateLimited {
            cooldown_secs(retry_after)
        } else {
            backoff_secs(2)
        };
        let next = self
            .state
            .fail(did, transient, cooldown, detail, self.fallback_for(source))?;
        if transient {
            self.progress
                .repos_failed_transient
                .fetch_add(1, Ordering::Relaxed);
        } else {
            self.progress
                .repos_failed_terminal
                .fetch_add(1, Ordering::Relaxed);
        }
        if next == super::state::RepoState::Terminal {
            metrics::BACKFILL_REPOS_TERMINAL_TOTAL.inc();
        }
        tracing::debug!(
            did,
            source,
            class = class.as_str(),
            next = next.as_str(),
            detail,
            "backfill fetch failed"
        );
        Ok(())
    }

    /// Run one source's fetch worker until it has nothing to do for
    /// `idle_exit`, or the host trips its cooldown, or shutdown.
    pub async fn fetch_worker(self: &Arc<Self>, source_name: &str) -> Result<(), RunnerError> {
        if source_name == HUBBLE_SOURCE {
            let Some(hubble) = self.hubble.clone() else {
                return Ok(());
            };
            let health = Arc::new(HostHealth::new(
                self.cfg.hubble_concurrency,
                self.cfg.hubble_concurrency,
                true,
            ));
            return self.fetch_loop(hubble.as_ref(), health).await;
        }
        let host = Hostname::new(source_name)
            .map_err(|e| RunnerError::Client(format!("bad source host: {e}")))?;
        let stored = self.state.host(&host)?.and_then(|h| h.concurrency);
        let limits = self.cfg.policy.limits_for(&host);
        let health = Arc::new(HostHealth::from_limits(limits, stored));
        let source = self.pds_source(&host);
        self.fetch_loop(&source, health).await
    }

    async fn fetch_loop<S: RepoSource + Clone + 'static>(
        self: &Arc<Self>,
        source: &S,
        health: Arc<HostHealth>,
    ) -> Result<(), RunnerError> {
        let name = source.name().to_owned();
        let class_label = source_class(&name);
        let mut in_flight: JoinSet<(String, String, JobOutcome)> = JoinSet::new();
        let mut queue: std::collections::VecDeque<(String, String)> =
            std::collections::VecDeque::new();
        let mut idle_since: Option<Instant> = None;
        tracing::info!(source = %name, limit = health.limit(), "fetch worker started");

        loop {
            if SHUTDOWN.load(Ordering::Relaxed) {
                break;
            }
            if let Some(new_limit) = health.maybe_recover(Instant::now()) {
                tracing::debug!(source = %name, new_limit, "recovered fetch concurrency");
            }

            // Settle anything finished, and wait if we are at the ceiling.
            while let Some(joined) = in_flight.try_join_next() {
                let (did, rev, outcome) = joined?;
                self.settle(&name, class_label, &health, did, rev, outcome)?;
            }
            while in_flight.len() >= health.limit() {
                if let Some(joined) = in_flight.join_next().await {
                    let (did, rev, outcome) = joined?;
                    self.settle(&name, class_label, &health, did, rev, outcome)?;
                }
            }

            if health.triggered() {
                let secs = health.cooldown_secs();
                metrics::BACKFILL_HOST_COOLDOWNS_TOTAL.inc();
                tracing::warn!(source = %name, cooldown_secs = secs, "concurrency floor exhausted; cooling down");
                // Return anything we claimed but did not start.
                for (did, _) in std::mem::take(&mut queue) {
                    self.state.fail(&did, true, secs, "host cooldown", None)?;
                }
                if name == HUBBLE_SOURCE {
                    sleep_secs(secs).await;
                } else if let Ok(host) = Hostname::new(&name) {
                    self.state
                        .set_host_cooldown(&host, secs, "concurrency floor exhausted")?;
                }
                break;
            }

            // The demand gate.
            if !self.sink.has_capacity() {
                metrics::BACKFILL_GATE_WAITS_TOTAL.inc();
                self.progress.gate_waits.fetch_add(1, Ordering::Relaxed);
                tokio::time::sleep(self.cfg.gate_poll).await;
                continue;
            }

            if queue.is_empty() {
                let want = health.limit().max(1) * self.cfg.claim_multiplier.max(1);
                let claimed = self.state.claim_for_source(&name, want)?;
                if claimed.is_empty() {
                    if in_flight.is_empty() {
                        let since = *idle_since.get_or_insert_with(Instant::now);
                        if since.elapsed() >= self.cfg.idle_exit {
                            break;
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    continue;
                }
                idle_since = None;
                queue.extend(claimed);
            }

            let Some((did, rev)) = queue.pop_front() else {
                continue;
            };
            let this = Arc::clone(self);
            let src = source.clone();
            metrics::BACKFILL_IN_FLIGHT_FETCHES.inc();
            in_flight.spawn(async move {
                let outcome = this.fetch_one(&src, &did).await;
                metrics::BACKFILL_IN_FLIGHT_FETCHES.dec();
                (did, rev, outcome)
            });
        }

        // Drain in-flight work and return unstarted claims.
        while let Some(joined) = in_flight.join_next().await {
            let (did, rev, outcome) = joined?;
            self.settle(&name, class_label, &health, did, rev, outcome)?;
        }
        for (did, _) in std::mem::take(&mut queue) {
            self.state.fail(&did, true, 0, "worker exit", None)?;
        }
        tracing::info!(source = %name, "fetch worker stopped");
        Ok(())
    }

    fn settle(
        self: &Arc<Self>,
        source: &str,
        class_label: &'static str,
        health: &Arc<HostHealth>,
        did: String,
        _listed_rev: String,
        outcome: JobOutcome,
    ) -> Result<(), RunnerError> {
        match outcome {
            JobOutcome::Accepted {
                receipt,
                bytes,
                spilled,
            } => {
                health.record(None, None);
                metrics::BACKFILL_REPOS_FETCHED_TOTAL
                    .with_label_values(&[class_label])
                    .inc();
                metrics::BACKFILL_BYTES_FETCHED_TOTAL
                    .with_label_values(&[class_label])
                    .inc_by(bytes);
                self.progress.repos_fetched.fetch_add(1, Ordering::Relaxed);
                self.progress.bytes.fetch_add(bytes, Ordering::Relaxed);
                self.progress
                    .records
                    .fetch_add(receipt.records as u64, Ordering::Relaxed);
                if spilled {
                    self.progress.spilled.fetch_add(1, Ordering::Relaxed);
                }
                // Mark the repo done only once its batch has committed. The
                // wait lives in its own task so a slow batch does not hold a
                // fetch slot.
                let this = Arc::clone(self);
                let source = source.to_owned();
                tokio::spawn(async move {
                    let Receipt { rev, committed, .. } = receipt;
                    match committed.await {
                        Ok(Ok(())) => {
                            if let Err(e) = this.state.complete(&did, &rev) {
                                tracing::error!(did, error = %e, "state: complete failed");
                            }
                            this.progress.repos_done.fetch_add(1, Ordering::Relaxed);
                            metrics::BACKFILL_REPOS_DONE_TOTAL.inc();
                        }
                        Ok(Err(e)) => {
                            tracing::warn!(did, error = %e, "batch failed; repo returned for retry");
                            drop(this.state.fail(
                                &did,
                                true,
                                backoff_secs(2),
                                &e.to_string(),
                                None,
                            ));
                        }
                        Err(_) => {
                            drop(this.state.fail(&did, true, 0, "sink dropped", None));
                        }
                    }
                    drop(source);
                });
                Ok(())
            }
            JobOutcome::Failed {
                class,
                retry_after,
                detail,
            } => self.settle_failure(source, health, &did, class, retry_after, &detail),
        }
    }

    // ----------------------------------------------------------- coordinator

    /// Which sources are eligible to run right now.
    fn eligible_sources(&self) -> Result<Vec<String>, RunnerError> {
        let now = chrono::Utc::now().timestamp();
        let mut out = Vec::new();
        if self.hubble.is_some() && self.state.has_claimable(HUBBLE_SOURCE)? {
            out.push(HUBBLE_SOURCE.to_owned());
        }
        if self.cfg.direct {
            for h in self.state.direct_hosts()? {
                if h.cooldown_until > now {
                    continue;
                }
                if self.state.has_claimable(h.host.as_str())? {
                    out.push(h.host.as_str().to_owned());
                }
            }
        }
        Ok(out)
    }

    /// Keep one fetch worker running per source with claimable work. Returns
    /// when `until_idle` is set and enumeration is done with nothing left to
    /// claim or in flight, or on shutdown.
    pub async fn drain(self: &Arc<Self>, until_idle: bool) -> Result<(), RunnerError> {
        let mut active: JoinSet<(String, Result<(), RunnerError>)> = JoinSet::new();
        let mut active_names: HashSet<String> = HashSet::new();
        let mut last_progress = Instant::now();
        let mut last = ProgressSnapshot::default();
        let started = Instant::now();

        loop {
            if SHUTDOWN.load(Ordering::Relaxed) {
                break;
            }
            while let Some(joined) = active.try_join_next() {
                let (name, result) = joined?;
                active_names.remove(&name);
                if let Err(e) = result {
                    tracing::error!(source = %name, error = %e, "fetch worker failed");
                }
            }

            let eligible = self.eligible_sources()?;
            for name in &eligible {
                if active.len() >= self.cfg.max_workers || active_names.contains(name) {
                    continue;
                }
                active_names.insert(name.clone());
                let this = Arc::clone(self);
                let name = name.clone();
                active.spawn(async move {
                    let r = this.fetch_worker(&name).await;
                    (name, r)
                });
            }
            metrics::BACKFILL_ACTIVE_WORKERS.set(i64::try_from(active.len()).unwrap_or(i64::MAX));

            if last_progress.elapsed() >= self.cfg.progress_every {
                last = self.log_progress(&last, started, active.len());
                last_progress = Instant::now();
            }

            if until_idle
                && self.progress.enumeration_done.load(Ordering::Relaxed)
                && eligible.is_empty()
                && active.is_empty()
            {
                break;
            }
            tokio::time::sleep(self.cfg.coordinator_poll).await;
        }

        while let Some(joined) = active.join_next().await {
            let (name, result) = joined?;
            if let Err(e) = result {
                tracing::error!(source = %name, error = %e, "fetch worker failed");
            }
        }
        self.log_progress(&last, started, 0);
        Ok(())
    }

    fn log_progress(
        &self,
        last: &ProgressSnapshot,
        started: Instant,
        workers: usize,
    ) -> ProgressSnapshot {
        let now = ProgressSnapshot::of(&self.progress);
        let dt = self.cfg.progress_every.as_secs_f64().max(1.0);
        #[allow(clippy::cast_precision_loss)]
        {
            tracing::info!(
                workers,
                fetched = now.fetched,
                done = now.done,
                failed_transient = now.failed_transient,
                failed_terminal = now.failed_terminal,
                repos_per_s = format!("{:.1}", (now.fetched - last.fetched) as f64 / dt),
                records_per_s = format!("{:.0}", (now.records - last.records) as f64 / dt),
                mb_per_s = format!("{:.2}", (now.bytes - last.bytes) as f64 / dt / 1_048_576.0),
                total_gb = format!("{:.2}", now.bytes as f64 / 1_073_741_824.0),
                gate_waits = now.gate_waits,
                uptime_s = started.elapsed().as_secs(),
                "backfill progress"
            );
        }
        super::sample_state_metrics(&self.state);
        now
    }

    /// The whole thing: enumerate (and re-enumerate if configured) while
    /// draining. Returns when everything is fetched, or never if
    /// re-enumeration is on, or on shutdown.
    pub async fn run(self: Arc<Self>) -> Result<(), RunnerError> {
        let recovered = self.state.reset_claimed()?;
        if recovered > 0 {
            tracing::info!(recovered, "returned claimed rows to pending after restart");
        }

        let enumerator = {
            let this = Arc::clone(&self);
            tokio::spawn(async move {
                loop {
                    if let Err(e) = this.enumerate_all().await {
                        tracing::error!(error = %e, "enumeration failed; will retry after backoff");
                        sleep_secs(60).await;
                        if SHUTDOWN.load(Ordering::Relaxed) {
                            return;
                        }
                        continue;
                    }
                    let Some(after) = this.cfg.reenumerate_after else {
                        return;
                    };
                    let deadline = Instant::now() + after;
                    while Instant::now() < deadline {
                        if SHUTDOWN.load(Ordering::Relaxed) {
                            return;
                        }
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                    if let Err(e) = this.state.reset_enumeration() {
                        tracing::error!(error = %e, "reset enumeration failed");
                    }
                    this.progress
                        .enumeration_done
                        .store(false, Ordering::Relaxed);
                    metrics::BACKFILL_ENUMERATION_COMPLETE.set(0);
                }
            })
        };

        let until_idle = self.cfg.reenumerate_after.is_none();
        let result = self.drain(until_idle).await;
        if !until_idle || SHUTDOWN.load(Ordering::Relaxed) {
            enumerator.abort();
        } else {
            drop(enumerator.await);
        }
        result
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct ProgressSnapshot {
    fetched: u64,
    done: u64,
    failed_transient: u64,
    failed_terminal: u64,
    records: u64,
    bytes: u64,
    gate_waits: u64,
}

impl ProgressSnapshot {
    fn of(p: &Progress) -> Self {
        Self {
            fetched: p.repos_fetched.load(Ordering::Relaxed),
            done: p.repos_done.load(Ordering::Relaxed),
            failed_transient: p.repos_failed_transient.load(Ordering::Relaxed),
            failed_terminal: p.repos_failed_terminal.load(Ordering::Relaxed),
            records: p.records.load(Ordering::Relaxed),
            bytes: p.bytes.load(Ordering::Relaxed),
            gate_waits: p.gate_waits.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::significant_drop_tightening)]
    use super::*;
    use crate::backfiller::source::{RepoBody, RepoPage, RepoRef};
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;
    use tokio::sync::oneshot;

    type CannedFetches = Arc<Mutex<Vec<Result<Vec<u8>, SourceError>>>>;

    /// Serves canned pages and canned fetch results.
    #[derive(Clone)]
    struct FakeSource {
        name: String,
        pages: Arc<Mutex<Vec<RepoPage>>>,
        fetches: CannedFetches,
    }

    impl FakeSource {
        fn new(
            name: &str,
            pages: Vec<RepoPage>,
            fetches: Vec<Result<Vec<u8>, SourceError>>,
        ) -> Self {
            Self {
                name: name.into(),
                pages: Arc::new(Mutex::new(pages)),
                fetches: Arc::new(Mutex::new(fetches)),
            }
        }
    }

    impl RepoSource for FakeSource {
        fn name(&self) -> &str {
            &self.name
        }
        async fn list_repos(
            &self,
            _cursor: Option<String>,
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
            _did: String,
            _limits: FetchLimits,
        ) -> Result<RepoBody, SourceError> {
            let mut f = self.fetches.lock().unwrap();
            if f.is_empty() {
                return Err(SourceError::Transport("no canned fetch".into()));
            }
            f.remove(0).map(RepoBody::Memory)
        }
    }

    /// Accepts everything without parsing; reports a caller-controlled room.
    struct FakeSink {
        room: AtomicBool,
        ingested: AtomicUsize,
        fail_commit: AtomicBool,
    }

    impl FakeSink {
        fn new(room: bool) -> Arc<Self> {
            Arc::new(Self {
                room: AtomicBool::new(room),
                ingested: AtomicUsize::new(0),
                fail_commit: AtomicBool::new(false),
            })
        }
    }

    impl RecordSink for FakeSink {
        fn has_capacity(&self) -> bool {
            self.room.load(Ordering::Relaxed)
        }
        async fn ingest(&self, _did: &str, body: RepoBody) -> Result<Receipt, WintermuteError> {
            if body.is_empty() {
                return Err(WintermuteError::Repo("empty".into()));
            }
            self.ingested.fetch_add(1, Ordering::Relaxed);
            let (tx, rx) = oneshot::channel();
            drop(tx.send(if self.fail_commit.load(Ordering::Relaxed) {
                Err(WintermuteError::Other("boom".into()))
            } else {
                Ok(())
            }));
            Ok(Receipt {
                records: 10,
                filtered: 0,
                rev: "3lz7gd2xq5c2c".into(),
                committed: rx,
            })
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
            hubble: None,
            direct: true,
            idle_exit: Duration::from_millis(50),
            coordinator_poll: Duration::from_millis(20),
            gate_poll: Duration::from_millis(10),
            list_max_retries: 1,
            ..RunnerConfig::default()
        }
    }

    fn runner(sink: Arc<FakeSink>) -> Arc<Runner<FakeSink>> {
        Arc::new(Runner::new(cfg(), sink, RepoStateStore::open_in_memory().unwrap()).unwrap())
    }

    const HOST: &str = "morel.us-east.host.bsky.network";

    async fn settle_all() {
        // Completion tasks are spawned; give them a tick.
        tokio::time::sleep(Duration::from_millis(30)).await;
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
        let r = runner(FakeSink::new(true));
        let src = FakeSource::new(HOST, pages, vec![]);
        let rep = r.enumerate_pages(&src, None).await.unwrap();
        assert_eq!((rep.pages, rep.seen, rep.needs_fetch), (2, 3, 3));
        assert!(rep.completed);
        assert_eq!(r.state().get_cursor(&format!("enum:{HOST}")).unwrap(), None);
    }

    #[tokio::test]
    async fn enumeration_persists_a_did_cursor_and_resumes_from_it() {
        let pages = vec![RepoPage {
            repos: vec![repo("did:a", "r1")],
            cursor: Some("did:plc:222rpxpbdd4cd2opywsqpxtz".into()),
        }];
        let r = runner(FakeSink::new(true));
        let src = FakeSource::new("hubble", pages, vec![]);
        let rep = r.enumerate_pages(&src, Some(1)).await.unwrap();
        assert!(!rep.completed);
        assert_eq!(
            r.state().get_cursor("enum:hubble").unwrap().as_deref(),
            Some("did:plc:222rpxpbdd4cd2opywsqpxtz")
        );
    }

    #[tokio::test]
    async fn a_repeated_cursor_ends_the_pass() {
        let r = runner(FakeSink::new(true));
        r.state().set_cursor("enum:fake", "stuck").unwrap();
        let pages = vec![RepoPage {
            repos: vec![repo("did:a", "r1")],
            cursor: Some("stuck".into()),
        }];
        let src = FakeSource::new("fake", pages, vec![]);
        let rep = r.enumerate_pages(&src, None).await.unwrap();
        assert_eq!(rep.pages, 1);
        assert!(rep.completed, "a loop is treated as the end");
    }

    #[tokio::test]
    async fn enumeration_retries_transient_page_failures_and_surfaces_terminal_ones() {
        #[derive(Clone)]
        struct Flaky(Arc<AtomicUsize>, u16);
        impl RepoSource for Flaky {
            fn name(&self) -> &'static str {
                "flaky"
            }
            async fn list_repos(
                &self,
                _c: Option<String>,
                _l: u32,
            ) -> Result<RepoPage, SourceError> {
                if self.0.fetch_add(1, Ordering::Relaxed) == 0 {
                    return Err(SourceError::Status {
                        status: self.1,
                        retry_after_secs: Some(0),
                        xrpc_error: false,
                    });
                }
                Ok(RepoPage {
                    repos: vec![repo("did:a", "r1")],
                    cursor: None,
                })
            }
            async fn fetch_repo(
                &self,
                _d: String,
                _l: FetchLimits,
            ) -> Result<RepoBody, SourceError> {
                unreachable!()
            }
        }
        let r = runner(FakeSink::new(true));
        let rep = r
            .enumerate_pages(&Flaky(Arc::new(AtomicUsize::new(0)), 503), None)
            .await
            .unwrap();
        assert_eq!(rep.seen, 1, "retried once and completed");

        let err = r
            .enumerate_pages(&Flaky(Arc::new(AtomicUsize::new(0)), 404), None)
            .await
            .unwrap_err();
        assert!(matches!(err, RunnerError::Source(_)));
    }

    #[tokio::test]
    async fn a_worker_fetches_claimed_repos_and_marks_them_done_at_the_archive_rev() {
        let sink = FakeSink::new(true);
        let r = runner(Arc::clone(&sink));
        let host = Hostname::new(HOST).unwrap();
        r.state().upsert_host(&host, 5, None, true).unwrap();
        r.state().upsert(&repo("did:a", "r1"), HOST, false).unwrap();
        r.state().upsert(&repo("did:b", "r1"), HOST, false).unwrap();
        let src = FakeSource::new(HOST, vec![], vec![Ok(vec![1]), Ok(vec![2])]);
        let health = Arc::new(HostHealth::new(2, 2, true));
        r.fetch_loop(&src, health).await.unwrap();
        settle_all().await;

        assert_eq!(sink.ingested.load(Ordering::Relaxed), 2);
        let st = r.state().stats().unwrap();
        assert_eq!((st.done, st.pending), (2, 0));
        assert_eq!(r.progress.repos_done.load(Ordering::Relaxed), 2);
        // The rev recorded is the CAR's, not the listing's.
        assert_eq!(
            r.state()
                .upsert(&repo("did:a", "3lz7gd2xq5c2b"), HOST, false)
                .unwrap(),
            crate::backfiller::state::Upsert::Unchanged
        );
    }

    #[tokio::test]
    async fn the_demand_gate_claims_nothing_while_the_sink_is_full() {
        let sink = FakeSink::new(false);
        let r = runner(Arc::clone(&sink));
        r.state().upsert(&repo("did:a", "r1"), HOST, false).unwrap();
        let src = FakeSource::new(HOST, vec![], vec![Ok(vec![1])]);
        let health = Arc::new(HostHealth::new(2, 2, true));

        let this = Arc::clone(&r);
        let handle = tokio::spawn(async move { this.fetch_loop(&src, health).await });
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert_eq!(sink.ingested.load(Ordering::Relaxed), 0);
        assert_eq!(
            r.state().stats().unwrap().pending,
            1,
            "still pending, not claimed"
        );
        assert!(r.progress.gate_waits.load(Ordering::Relaxed) > 0);

        sink.room.store(true, Ordering::Relaxed);
        handle.await.unwrap().unwrap();
        settle_all().await;
        assert_eq!(sink.ingested.load(Ordering::Relaxed), 1);
        assert_eq!(r.state().stats().unwrap().done, 1);
    }

    #[tokio::test]
    async fn a_transient_failure_returns_the_repo_to_pending_and_a_terminal_one_writes_it_off() {
        let r = runner(FakeSink::new(true));
        r.state().upsert(&repo("did:a", "r1"), HOST, false).unwrap();
        r.state().upsert(&repo("did:b", "r1"), HOST, false).unwrap();
        let src = FakeSource::new(
            HOST,
            vec![],
            vec![
                Err(SourceError::Status {
                    status: 503,
                    retry_after_secs: None,
                    xrpc_error: false,
                }),
                Err(SourceError::Status {
                    status: 404,
                    retry_after_secs: None,
                    xrpc_error: false,
                }),
            ],
        );
        let health = Arc::new(HostHealth::new(1, 1, true));
        r.fetch_loop(&src, health).await.unwrap();
        let st = r.state().stats().unwrap();
        assert_eq!((st.pending, st.terminal), (1, 1));
        assert_eq!(r.progress.repos_failed_transient.load(Ordering::Relaxed), 1);
        assert_eq!(r.progress.repos_failed_terminal.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn an_unparseable_archive_is_terminal_and_a_failed_commit_is_retried() {
        let sink = FakeSink::new(true);
        let r = runner(Arc::clone(&sink));
        r.state().upsert(&repo("did:a", "r1"), HOST, false).unwrap();
        let src = FakeSource::new(HOST, vec![], vec![Ok(vec![])]);
        r.fetch_loop(&src, Arc::new(HostHealth::new(1, 1, true)))
            .await
            .unwrap();
        assert_eq!(r.state().stats().unwrap().terminal, 1);

        sink.fail_commit.store(true, Ordering::Relaxed);
        r.state().upsert(&repo("did:b", "r1"), HOST, false).unwrap();
        let src = FakeSource::new(HOST, vec![], vec![Ok(vec![1])]);
        r.fetch_loop(&src, Arc::new(HostHealth::new(1, 1, true)))
            .await
            .unwrap();
        settle_all().await;
        assert_eq!(
            r.state().stats().unwrap().pending,
            1,
            "back to pending for retry"
        );
        assert_eq!(r.progress.repos_done.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn a_direct_host_that_will_not_serve_a_repo_hands_it_to_hubble() {
        let mut c = cfg();
        c.hubble = Some(HubbleConfig::default());
        let r = Arc::new(
            Runner::new(
                c,
                FakeSink::new(true),
                RepoStateStore::open_in_memory().unwrap(),
            )
            .unwrap(),
        );
        r.state().upsert(&repo("did:a", "r1"), HOST, false).unwrap();
        let src = FakeSource::new(
            HOST,
            vec![],
            vec![Err(SourceError::Status {
                status: 400,
                retry_after_secs: None,
                xrpc_error: true,
            })],
        );
        r.fetch_loop(&src, Arc::new(HostHealth::new(1, 1, true)))
            .await
            .unwrap();
        assert_eq!(
            r.state().source_of("did:a").unwrap().as_deref(),
            Some(HUBBLE_SOURCE)
        );
        assert_eq!(r.state().stats().unwrap().pending, 1);
        assert_eq!(r.fallback_for(HOST), Some(HUBBLE_SOURCE));
        assert_eq!(r.fallback_for(HUBBLE_SOURCE), None);
    }

    #[tokio::test]
    async fn a_streak_of_transient_errors_steps_the_host_down_then_cools_it() {
        let r = runner(FakeSink::new(true));
        let host = Hostname::new("small.example").unwrap();
        r.state().upsert_host(&host, 5, None, true).unwrap();
        let mut fetches = Vec::new();
        for i in 0..8 {
            r.state()
                .upsert(&repo(&format!("did:{i}"), "r1"), "small.example", false)
                .unwrap();
            fetches.push(Err(SourceError::Transport("connect".into())));
        }
        let src = FakeSource::new("small.example", vec![], fetches);
        // Start at 2 so a streak of 3 reduces to 1, then the next streak trips.
        let health = Arc::new(HostHealth::new(2, 2, true));
        r.fetch_loop(&src, Arc::clone(&health)).await.unwrap();
        assert!(health.triggered());
        let row = r.state().host(&host).unwrap().unwrap();
        assert_eq!(row.concurrency, Some(1), "reduction persisted");
        assert!(
            row.cooldown_until > chrono::Utc::now().timestamp(),
            "host parked"
        );
        // Unstarted claims went back to pending with the cooldown.
        assert_eq!(r.state().stats().unwrap().claimed, 0);
    }

    #[tokio::test]
    async fn the_coordinator_runs_a_worker_per_eligible_source_and_finishes_when_idle() {
        let sink = FakeSink::new(true);
        let r = runner(Arc::clone(&sink));
        // Two direct hosts with work; hubble not configured, so hubble rows are ignored.
        for h in ["a.host.bsky.network", "b.host.bsky.network"] {
            r.state()
                .upsert_host(&Hostname::new(h).unwrap(), 1, None, true)
                .unwrap();
            r.state()
                .upsert(&repo(&format!("did:{h}"), "r1"), h, false)
                .unwrap();
        }
        r.state()
            .upsert(&repo("did:h", "r1"), HUBBLE_SOURCE, false)
            .unwrap();
        assert_eq!(r.eligible_sources().unwrap().len(), 2);
        r.progress.enumeration_done.store(true, Ordering::Relaxed);
        // Real PdsSource against nowhere: fetches fail as transport errors and
        // get retried later, which is fine -- we only need the loop to exit.
        let this = Arc::clone(&r);
        tokio::time::timeout(
            Duration::from_secs(10),
            async move { this.drain(true).await },
        )
        .await
        .expect("drain exits when idle")
        .unwrap();
        let st = r.state().stats().unwrap();
        assert_eq!(st.claimed, 0, "nothing stranded");
    }

    #[tokio::test]
    async fn discovery_records_direct_hosts_and_extra_hosts() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/xrpc/com.atproto.sync.listHosts")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_body(
                r#"{"hosts":[
                {"hostname":"x.host.bsky.network","accountCount":10,"status":"active"},
                {"hostname":"y.host.bsky.network","accountCount":10,"status":"offline"},
                {"hostname":"tiny.example","accountCount":1,"status":"active"}]}"#,
            )
            .create_async()
            .await;
        let mut c = cfg();
        c.relay_url = Some(server.url());
        c.extra_hosts = vec![Hostname::new("blacksky.app").unwrap()];
        let r = Arc::new(
            Runner::new(
                c,
                FakeSink::new(true),
                RepoStateStore::open_in_memory().unwrap(),
            )
            .unwrap(),
        );
        assert_eq!(r.discover().await.unwrap(), 2);
        let direct: Vec<String> = r
            .state()
            .direct_hosts()
            .unwrap()
            .into_iter()
            .map(|h| h.host.as_str().to_owned())
            .collect();
        assert_eq!(direct, vec!["x.host.bsky.network", "blacksky.app"]);

        let mut off = cfg();
        off.direct = false;
        off.relay_url = Some(server.url());
        let r = Arc::new(
            Runner::new(
                off,
                FakeSink::new(true),
                RepoStateStore::open_in_memory().unwrap(),
            )
            .unwrap(),
        );
        assert_eq!(r.discover().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn enumerate_host_marks_progress_by_outcome() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/xrpc/com.atproto.sync.listRepos")
            .match_query(mockito::Matcher::Any)
            .with_status(404)
            .create_async()
            .await;
        let r = runner(FakeSink::new(true));
        // enumerate_host builds a real PdsSource for the host name, which will
        // not resolve; use the runner's page walker directly against a fake
        // for the success path and check the terminal path via a host with
        // no DNS (transport => transient => cooldown).
        let host = Hostname::new("does-not-resolve.invalid").unwrap();
        r.state().upsert_host(&host, 1, None, true).unwrap();
        let row = r.state().host(&host).unwrap().unwrap();
        r.enumerate_host(&row).await.unwrap();
        let row = r.state().host(&host).unwrap().unwrap();
        assert_eq!(row.list_state, ListState::Pending);
        assert!(
            row.cooldown_until > chrono::Utc::now().timestamp(),
            "transport => cooldown"
        );
        // Cooling hosts are skipped.
        r.enumerate_host(&row).await.unwrap();

        let done = FakeSource::new(
            "done.example",
            vec![RepoPage {
                repos: vec![repo("did:a", "r1")],
                cursor: None,
            }],
            vec![],
        );
        let rep = r.enumerate_pages(&done, None).await.unwrap();
        assert!(rep.completed);
        assert_eq!(source_class(HUBBLE_SOURCE), "hubble");
        assert_eq!(source_class(HOST), "bsky");
        assert_eq!(source_class("tiny.example"), "pds");
    }

    #[tokio::test]
    async fn run_enumerates_then_drains_to_idle_when_not_reenumerating() {
        let r = runner(FakeSink::new(true));
        // No relay, no hosts, no hubble: enumeration is a no-op and drain
        // exits immediately once it observes enumeration_done.
        let this = Arc::clone(&r);
        tokio::time::timeout(Duration::from_secs(5), this.run())
            .await
            .expect("run exits")
            .unwrap();
        assert!(r.progress.enumeration_done.load(Ordering::Relaxed));
    }
}
