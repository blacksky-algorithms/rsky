//! top-level entry to hubble-sync!

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::cancel::CancelExt;
use crate::crawl_strategy_list_repos::{
    CrawlError, crawl_upstream_listrepos, deep_crawl_listhosts_listrepos,
};
use crate::metrics::{
    BIG_REPO_PERMITS_IN_USE, BIG_REPO_PERMITS_LIMIT, REPOS_BY_ACCOUNT_STATUS,
    REPOS_DESYNC_BY_REASON, REPOS_TOTAL,
};
use crate::pending_identity_scheduler;
use crate::rescope_sweep;
use crate::resync_scheduler;
use crate::storage::repo::AccountStatusSource;

use crate::{
    CrawlState, FirehoseError, FirehoseSubscriber, HostRegistry, HubbleSyncResolver, LoadError,
    PendingScheduler, PrefixedEngine, RepoCountsByState, RepoCountsByStatus, RepoRegistry, Resolve,
    ResyncScheduler, StorageEngine, StorageError, SyncConfig, SyncConsumer, SyncHandle,
};

const POLLED_METRICS_INTERVAL: Duration = Duration::from_secs(15);
const ACTOR_DRAIN_TIMEOUT: Duration = Duration::from_secs(10);

/// top-level error for all main hubble tasks
#[derive(Debug, thiserror::Error)]
pub enum HubbleSyncError<E: StorageError> {
    #[error("firehose subscriber: {0}")]
    Firehose(#[from] FirehoseError<E>),

    #[error("resync scheduler: {0}")]
    ResyncScheduler(#[source] resync_scheduler::ResyncDriveError<E>),

    #[error("pending identity scheduler: {0}")]
    PendingIdentityScheduler(#[source] LoadError<E>),

    #[error("backfill repo discovery: {0}")]
    RepoDiscovery(#[source] CrawlError<E>),

    #[error("rescope sweep: {0}")]
    RescopeSweep(#[source] rescope_sweep::RescopeSweepError<E>),

    #[error("background task panicked")]
    TaskPanic(#[from] tokio::task::JoinError),
}

pub struct HubbleSync<S, A, R = HubbleSyncResolver>
where
    S: StorageEngine,
    A: SyncConsumer<Engine = S>,
    R: Resolve,
{
    cancel: CancellationToken,
    storage: PrefixedEngine<S>,
    hosts: Arc<HostRegistry>,
    repo_registry: Arc<RepoRegistry<S, A, R>>,
    resync_scheduler: Arc<ResyncScheduler>,
    pending_scheduler: Arc<PendingScheduler>,
    config: SyncConfig,
    shutdown: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
}

/// convenient default-resolver impl
///
/// TODO: should annoyingly require app to pass contact info for the user-agent
impl<S, A> HubbleSync<S, A, HubbleSyncResolver>
where
    S: StorageEngine,
    A: SyncConsumer<Engine = S>,
{
    /// set up hubble-sync!
    ///
    /// consumer_app: your sync app, implementing the [`SyncConsumer`] trait
    /// storage: where hubble-sync will maintain its state
    /// config: adjust sync limits and behaviour
    /// contact: how you can be reached (yes, it's required)
    ///
    /// `contact` will be templated into the user-agent string Hubble adds to
    /// all requests it makes to external hosts over the internet. people
    /// running PDSes might see it in their logs.
    pub fn new(consumer_app: Arc<A>, storage: S, contact: &str, config: &SyncConfig) -> Self {
        let cancel = CancellationToken::new();
        let ua = format!(
            "hubble-sync v{} from @microcosm.blue. contact: {contact}",
            env!("CARGO_PKG_VERSION")
        );
        let hosts = HostRegistry::new(
            config.hosts.clone(),
            config.upstream.clone(),
            &ua,
            cancel.clone(),
            config.sync_scope,
        );
        let resolver = HubbleSyncResolver::new(&ua, &config.plc_url, hosts.clone());
        Self::with_resolver(consumer_app, storage, hosts, resolver, cancel, config)
    }
}

impl<S, A, R> HubbleSync<S, A, R>
where
    S: StorageEngine,
    A: SyncConsumer<Engine = S>,
    R: Resolve,
{
    pub fn with_resolver(
        consumer_app: Arc<A>,
        storage: S,
        hosts: Arc<HostRegistry>,
        resolver: R,
        cancel: CancellationToken,
        config: &SyncConfig,
    ) -> Self {
        assert!(
            !(config.upstream.kind.is_relay() && config.upstream.get_repo_token.is_some()),
            "getRepo bearer token is only allowed for PDS upstream (found relay upstream)",
        );

        let storage = PrefixedEngine::new(storage, config.hubble_sync_storage_prefix);
        let resync_scheduler = Arc::new(ResyncScheduler::new());
        let pending_scheduler =
            Arc::new(PendingScheduler::new(config.pending_identity_queue_limit));
        let repo_registry = Arc::new(RepoRegistry::new(
            storage.clone(),
            hosts.clone(),
            Arc::new(resolver),
            consumer_app,
            cancel.clone(),
            config.clone(),
        ));
        Self {
            cancel,
            storage,
            hosts,
            repo_registry,
            resync_scheduler,
            pending_scheduler,
            config: config.clone(),
            shutdown: None,
        }
    }

    pub fn with_graceful_shutdown<F: Future<Output = ()> + Send + 'static>(
        mut self,
        signal: F,
    ) -> Self {
        self.shutdown = Some(Box::pin(signal));
        self
    }

    pub fn crawl_state(&self, strategy_id: &str) -> CrawlState<PrefixedEngine<S>> {
        CrawlState::new(self.storage.clone(), strategy_id)
    }

    /// Intake for repos discovered outside hubble-sync's own crawls (an
    /// external DID list, another index): request syncs through the same
    /// pending-identity path the upstream crawl uses. VENDORED ADDITION.
    pub fn discovery(&self) -> pending_identity_scheduler::RepoDiscovery<PrefixedEngine<S>> {
        pending_identity_scheduler::RepoDiscovery::new(
            self.storage.clone(),
            self.pending_scheduler.clone(),
        )
    }

    pub fn handle(&self) -> SyncHandle<S, A::CommitState, A::InfoState> {
        SyncHandle::new(
            self.storage.clone(),
            self.hosts.clone(),
            self.repo_registry.clone(),
        )
    }

    pub async fn run(self) -> Result<(), HubbleSyncError<S::Error>> {
        let mut tasks = JoinSet::new();

        // resync scheduler
        {
            let sched = self.resync_scheduler.clone();
            let stor = self.storage.clone();
            let hr = self.hosts.clone();
            let rr = self.repo_registry.clone();
            let qps = self.config.resync_dispatch_qps;
            let con = self.config.max_resync_concurrency;
            let c = self.cancel.clone();
            tasks.spawn(async move {
                resync_scheduler::drive(sched, stor, hr, rr, qps, con, c)
                    .await
                    .map_err(HubbleSyncError::ResyncScheduler)
            });
        }

        // pending identity scheduler driver
        {
            let permits = Arc::new(Semaphore::new(self.config.scheduled_resolve_limit.into()));
            let sched = self.pending_scheduler.clone();
            let stor = self.storage.clone();
            let rr = self.repo_registry.clone();
            let c = self.cancel.clone();
            tasks.spawn(async move {
                pending_identity_scheduler::drive(sched, permits, stor, rr, c)
                    .await
                    .map_err(HubbleSyncError::PendingIdentityScheduler)
            });
        }

        let upstream_host = self
            .hosts
            .get(&self.config.upstream.hostname)
            .expect("a valid subscribe host");

        // firehose subscriber
        {
            let host = upstream_host.clone();
            let registry = self.repo_registry.clone();
            let stor = self.storage.clone();
            let cfg = self.config.firehose.clone();
            let c = self.cancel.clone();
            tasks.spawn(async move {
                let mut subscriber = FirehoseSubscriber::new(host, registry, stor, cfg);
                subscriber.run(c).await.map_err(HubbleSyncError::Firehose)
            });
        }

        // backfill discovery driver (for now just upstream listrepos-crawl)
        {
            let upstream = upstream_host.clone();
            let stor = self.storage.clone();
            let qu = self.pending_scheduler.clone();
            let c = self.cancel.clone();
            let discovery = pending_identity_scheduler::RepoDiscovery::new(stor.clone(), qu);
            let crawl_prefix = format!("hubble-sync.upstream-listrepos.{}", upstream.name()); // TODO: let the crawl strategy declare its own prefix (partly)
            let crawl_state = self.crawl_state(&crawl_prefix);
            let recrawl_interval = Duration::from_secs(86_400);
            let status_source = if self.config.upstream.kind.is_relay() {
                AccountStatusSource::Relay(upstream.name().as_str().to_string())
            } else {
                AccountStatusSource::Pds(upstream.name().as_str().to_string())
            };
            tasks.spawn(async move {
                crawl_upstream_listrepos(
                    discovery,
                    crawl_state,
                    upstream,
                    status_source,
                    recrawl_interval,
                    c,
                )
                .await
                .map_err(HubbleSyncError::RepoDiscovery)
            });
        }
        // deep crawl, if we're doing that
        if let Some(deep) = self.config.deep_crawl.clone() {
            assert!(
                self.config.upstream.kind.is_relay(),
                "deep crawl only works with relay upstream"
            );
            let upstream = upstream_host.clone();
            let hosts = self.hosts.clone();
            let stor = self.storage.clone();
            let qu = self.pending_scheduler.clone();
            let c = self.cancel.clone();
            let discovery = pending_identity_scheduler::RepoDiscovery::new(stor.clone(), qu);
            let crawl_prefix = format!("hubble-sync.upstream-listrepos.{}", upstream.name()); // TODO: let the crawl strategy declare its own prefix (partly)
            tasks.spawn(async move {
                deep_crawl_listhosts_listrepos(
                    discovery,
                    hosts,
                    upstream_host.clone(),
                    stor,
                    crawl_prefix,
                    deep,
                    c,
                )
                .await
                .map_err(HubbleSyncError::RepoDiscovery)
            });
        }

        // one-shot rescope sweep (only widens)
        if self.config.rescope_sweep_on_start {
            let stor = self.storage.clone();
            let rr = self.repo_registry.clone();
            let c = self.cancel.clone();
            tasks.spawn(async move {
                rescope_sweep::run(stor, rr, c)
                    .await
                    .map_err(HubbleSyncError::RescopeSweep)
            });
        }

        // storage engine maintenance
        {
            let storage = self.storage.clone();
            let c = self.cancel.clone();
            tasks.spawn(async move {
                loop {
                    let storage = storage.clone();
                    // note: letting maintenance run without cancellation
                    match tokio::task::spawn_blocking(move || storage.maintenance())
                        .await
                        .expect("engine not to panic")
                    {
                        Ok(Some(wait)) => {
                            tracing::trace!(?wait, "storage maintenance requested re-run");
                            if !c.sleep(wait).await {
                                return Ok(());
                            }
                        }
                        Ok(None) => {
                            tracing::debug!("storage: no maintenance requested, stopping task");
                            return Ok(());
                        }
                        Err(err) => {
                            tracing::warn!(
                                ?err,
                                "storage maintenance problem, will try again in 15s"
                            );
                            if !c.sleep(Duration::from_secs(15)).await {
                                return Ok(());
                            }
                        }
                    }
                }
            });
        }

        // polled-gauge metrics
        {
            let storage = self.storage.clone();
            let rr = self.repo_registry.clone();
            let c = self.cancel.clone();
            tasks.spawn(async move {
                loop {
                    metrics::gauge!(BIG_REPO_PERMITS_IN_USE)
                        .set(rr.big_repo_permits_in_use() as f64);
                    metrics::gauge!(BIG_REPO_PERMITS_LIMIT).set(rr.big_repo_permit_limit() as f64);

                    let s = storage.clone();
                    let res =
                        tokio::task::spawn_blocking(move || -> Result<_, LoadError<S::Error>> {
                            let c = RepoCountsByState::load(&s)?;
                            let g = |state, n: u64| {
                                metrics::gauge!(REPOS_TOTAL, "state" => state).set(n as f64);
                            };
                            g("synchronized", c.synchronized);
                            g("desynchronized", c.desynchronized);
                            g("nonActive", c.non_active);
                            g("gone", c.gone);

                            for (reason, n) in &c.desynchronized_by_reason {
                                metrics::gauge!(REPOS_DESYNC_BY_REASON, "reason" => *reason)
                                    .set(*n as f64);
                            }

                            let c = RepoCountsByStatus::load(&s)?;
                            let g = |status, n: u64| {
                                metrics::gauge!(REPOS_BY_ACCOUNT_STATUS, "status" => status)
                                    .set(n as f64);
                            };
                            g("active", c.active);
                            g("deactivated", c.deactivated);
                            g("suspended", c.suspended);
                            g("takendown", c.takendown);
                            g("deleted", c.deleted);
                            g("other", c.other);

                            // engine-owned metrics
                            s.report_metrics();

                            Ok(())
                        })
                        .await
                        .expect("engine not to panic");

                    if let Err(err) = res {
                        error!(%err, "polling gauges failed (not stopping)");
                    }

                    if !c.sleep(POLLED_METRICS_INTERVAL).await {
                        return Ok(());
                    }
                }
            });
        }

        // supervisor / shutdown monitor
        {
            let mut shutdown_fut = if let Some(f) = self.shutdown {
                f
            } else {
                Box::pin(std::future::pending())
            };
            let mut shutdown_happening = false;
            let mut first_err = None;

            while !tasks.is_empty() {
                tokio::select! {
                    _ = shutdown_fut.as_mut(), if !shutdown_happening => {
                        info!("shutting down...");
                        self.cancel.cancel();
                        shutdown_happening = true;
                    }
                    Some(joined) = tasks.join_next() => match joined {
                        Ok(Ok(())) => {} // clean task shutdown
                        Ok(Err(e)) => {
                            if first_err.is_none() {
                                error!(error = ?e, "background task failed, shutting down...");
                                first_err = Some(e);
                                self.cancel.cancel();
                            } else {
                                warn!(error = ?e, "subsequent background task failure");
                            }
                        }
                        Err(join_err) => {
                            if first_err.is_none() {
                                error!(error = ?join_err, "background task panicked, shutting down...");
                                first_err = Some(join_err.into());
                                self.cancel.cancel();
                            } else {
                                warn!(error = ?join_err, "subsequent background task panic");
                            }
                        }
                    }
                }
            }

            // drain *after* anything sending into actors (firehose, resyncs,)
            // has finished.
            // TODO: does this get messed up when the consumer app gets a handle
            // that can also send in messages?
            self.repo_registry.drain(ACTOR_DRAIN_TIMEOUT).await;

            first_err.map(Err).unwrap_or(Ok(()))
        }
    }
}
