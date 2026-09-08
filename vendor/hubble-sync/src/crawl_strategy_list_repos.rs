//! repo discovery strategy via listRepos

use crate::pending_identity_scheduler::RepoDiscovery;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::{MissedTickBehavior, interval};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, trace, warn};

use crate::cancel::CancelExt;
use crate::host::{ListHostsResponse, PdsHost};
use crate::pending_identity_scheduler::DiscoveredRepo;
use crate::storage::DecodeError;
use crate::storage::repo::{AccountStatus, AccountStatusSource};
use crate::sync_handle::ListReposResponse;
use crate::{
    CrawlState, DeepCrawl, Host, HostRegistry, HostRequestError, LoadError, StorageBatch,
    StorageEngine, StorageError,
};

const LIST_REPOS_LIMIT: NonZeroUsize = NonZeroUsize::new(1000).unwrap();
const LIST_REPOS_RETRIES: u32 = 12;

const LIST_HOSTS_LIMIT: NonZeroUsize = NonZeroUsize::new(1000).unwrap();
const LIST_HOSTS_RETRIES: u32 = 12;

#[derive(Debug, thiserror::Error)]
pub enum CrawlError<E: StorageError> {
    #[error("storage: {0}")]
    Storage(#[source] E),
    #[error("load: {0}")]
    Load(#[from] LoadError<E>),
    #[error("decode: {0}")]
    Decode(#[from] DecodeError),
}

enum PassOutcome {
    Completed,
    Interrupted,
    Cancelled,
}

/// begin the crawl loop
///
/// runs forever: if you don't want to start app teardown if it exits, pass a
/// child token in
///
/// if there is a persisted cursor, it will be used on the *first* pass.
/// subsequent passes will always start fresh, even if the previous pass was
/// interrupted. This way, a restart mid-pass will resume, but a bad cursor
/// does not leave a host stranded.
pub async fn crawl_upstream_listrepos<S: StorageEngine>(
    discovery: RepoDiscovery<S>,
    state: CrawlState<S>,
    upstream: Arc<Host>,
    status_source: AccountStatusSource,
    recrawl_interval: Duration,
    cancel: CancellationToken,
) -> Result<(), CrawlError<S::Error>> {
    let mut recrawl = interval(recrawl_interval);
    recrawl.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let mut resume_cursor = true;

    loop {
        // once per ~day (recrawl duration)
        debug!(
            ?upstream,
            ?recrawl_interval,
            "discover crawl outer loop about to tick"
        );
        let Some(_) = cancel.run(recrawl.tick()).await else {
            return Ok(());
        };
        debug!(?upstream, ?recrawl_interval, "discover crawl proceeding");

        match crawl_one_pass(
            &discovery,
            &state,
            &upstream,
            &status_source,
            resume_cursor,
            &cancel,
        )
        .await?
        {
            PassOutcome::Cancelled => return Ok(()),
            PassOutcome::Completed => {
                // unconditionally make sure we've marked this if we complete
                mark_first_pass_completed(&state)
                    .await
                    .map_err(CrawlError::Storage)?;
            }
            PassOutcome::Interrupted => {
                debug!(?upstream, "crawl interrupted, will retry on next tick");
            }
        }
        resume_cursor = false;
    }
}

async fn mark_first_pass_completed<S: StorageEngine>(
    state: &CrawlState<S>,
) -> Result<(), S::Error> {
    let state = state.clone();
    tokio::task::spawn_blocking(move || {
        let mut b = state.batch();
        state.put(b"done_first_pass", b"1", &mut b);
        b.commit()
    })
    .await
    .expect("no task panic")
}

async fn check_first_pass_completed<S: StorageEngine>(
    state: &CrawlState<S>,
) -> Result<bool, S::Error> {
    let state = state.clone();
    tokio::task::spawn_blocking(move || Ok(state.get(b"done_first_pass")?.is_some()))
        .await
        .expect("no task panic")
}

pub async fn deep_crawl_listhosts_listrepos<S: StorageEngine>(
    discovery: RepoDiscovery<S>,
    registry: Arc<HostRegistry>,
    relay: Arc<Host>,
    storage: S,
    relay_strategy_id: String,
    config: DeepCrawl,
    cancel: CancellationToken,
) -> Result<(), CrawlError<S::Error>> {
    if config.wait_for_upstream_crawl {
        debug!(?relay, "waiting for upstream first pass...");
        let relay_state = CrawlState::new(storage.clone(), &relay_strategy_id);
        while !check_first_pass_completed(&relay_state)
            .await
            .map_err(CrawlError::Storage)?
        {
            trace!(?relay, "upstream not yet completed, sleeping then retrying");
            if !cancel.sleep(Duration::from_secs(30)).await {
                return Ok(()); // cancelled
            }
        }
        debug!(?relay, "upstream first pass completed");
    }

    let mut resume_cursor = true;

    loop {
        let pdses = list_all_hosts::<S>(&relay, &cancel).await?;
        let pdses = crawlable_hosts_sorted(pdses);
        debug!(count = pdses.len(), "deep crawl found active hosts");

        let limit = Arc::new(Semaphore::new(usize::from(config.concurrency)));
        let mut set: JoinSet<Result<PassOutcome, CrawlError<S::Error>>> = JoinSet::new();
        for h in pdses {
            let Some(permit) = cancel
                .run(limit.clone().acquire_owned())
                .await
                .transpose()
                .expect("semaphore not closed")
            else {
                return Ok(()); // cancelled
            };
            let pds = match registry.get(&h.hostname) {
                Ok(p) => p,
                Err(err) => {
                    warn!(host = %h.hostname, %err, "skipping bad hostname");
                    continue;
                }
            };
            let discovery = discovery.clone();
            let cancel = cancel.clone();
            let pds_state = CrawlState::new(
                storage.clone(),
                &format!("hubble-sync.upstream-listrepos.deep.{}", h.hostname),
            );
            let source = AccountStatusSource::Pds(h.hostname); // string for now, bleh
            set.spawn(async move {
                #[expect(unused_variables, reason = "concurrency permit moved into task")]
                let p = permit;
                crawl_one_pass(
                    &discovery,
                    &pds_state,
                    &pds,
                    &source,
                    resume_cursor,
                    &cancel,
                )
                .await
            });
        }
        while let Some(joined) = set.join_next().await {
            joined.expect("deep-crawl tasks not to panic")?; // storage errors abort
        }

        if !cancel.sleep(config.recrawl_interval).await {
            return Ok(());
        }
        resume_cursor = false;
    }
}

/// loop over getRepos pages for batches of repos
async fn crawl_one_pass<S: StorageEngine>(
    discovery: &RepoDiscovery<S>,
    state: &CrawlState<S>,
    upstream: &Host,
    status_source: &AccountStatusSource,
    resume: bool,
    cancel: &CancellationToken,
) -> Result<PassOutcome, CrawlError<S::Error>> {
    let s = state.clone();
    let mut cursor: Option<String> = tokio::task::spawn_blocking(move || s.get(b"cursor"))
        .await
        .expect("task not to panic")
        .map_err(CrawlError::Storage)?
        .map(|ref c| str::from_utf8(c).map(str::to_string))
        .transpose()
        .map_err(|err| DecodeError::NotUtf8 {
            what: "cursor",
            err,
        })?
        .filter(|_| resume);

    let mut i = 1;
    loop {
        debug!(?upstream, ?cursor, "fetching page");
        let Some(maybe_page) = cancel
            .run(list_repos_page(upstream, cursor.as_deref()))
            .await
        else {
            // cancelled
            return Ok(PassOutcome::Cancelled);
        };

        let Some(page) = maybe_page else {
            // failed to get upstream page. give up until the next go-round
            // note: existing cursor will be preserved! and might never get
            // reset if it was bad in an always-fails way (TODO)
            return Ok(PassOutcome::Interrupted);
        };

        let accounts = page
            .repos
            .into_iter()
            .map(|r| DiscoveredRepo {
                moderation: if r.active {
                    None
                } else {
                    Some((
                        AccountStatus::from_list_status(r.status),
                        status_source.clone(),
                    ))
                },
                did: r.did,
            })
            .collect();

        let Some(_) = cancel
            .run(discovery.request_sync(accounts, |b| match &page.cursor {
                Some(cursor) => state.put(b"cursor", cursor.as_bytes(), b),
                None => state.delete(b"cursor", b),
            }))
            .await
            .transpose()?
        else {
            return Ok(PassOutcome::Cancelled);
        };

        match page.cursor {
            None => {
                trace!(page = i, "finished paging listRepos");
                return Ok(PassOutcome::Completed);
            }
            Some(c) if Some(&c) == cursor.as_ref() => {
                debug!(
                    page = i,
                    cursor = c,
                    "apparent trivial cursor loop in response, breaking out"
                );
                return Ok(PassOutcome::Interrupted);
            }
            Some(c) if i >= 1_000_000 => {
                // TODO: on a PDS we can probably set this around 10k (~10M repos)
                // (largest known PDS is still under 1M)
                // For now (relay) 1M pages -> 1B repos (network approaching 50M repos)
                debug!(
                    page = i,
                    cursor = c,
                    "too many pages, breaking out of listRepos paging"
                );
                // cursor preserved........
                return Ok(PassOutcome::Interrupted);
            }
            Some(c) => {
                trace!(page = i, cursor = c, "listRepos continuing on next page");
                cursor = Some(c);
                i += 1;
            }
        }
    }
}

/// get one page from an upstream via com.atproto.sync.listRepos
///
/// calls upstream through `Host` inherit its self-throttling
async fn list_repos_page(host: &Host, cursor: Option<&str>) -> Option<ListReposResponse> {
    let name = host.name().as_str();

    for retry in 1_u32..=LIST_REPOS_RETRIES {
        match host.list_repos(LIST_REPOS_LIMIT, cursor).await {
            Ok(r) => return Some(r),
            Err(HostRequestError::HostBackoff { remaining }) => {
                trace!(?remaining, retry, host = name, "host backing off, waiting");
                tokio::time::sleep(remaining).await;
            }
            Err(
                err @ (HostRequestError::RateLimited { .. } | HostRequestError::ServerTransient),
            ) => {
                // (backoff is handled by the `Host` directly)
                trace!(%err, retry, host = name, "backoff response, retrying");
            }
            Err(err @ (HostRequestError::Transport(_) | HostRequestError::Timeout)) => {
                let backoff_secs = (3 + 2_u64.saturating_pow(retry)).min(900);
                trace!(%err, retry, backoff_secs, host = name, "upstream listRepos failed, retrying");
                tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
            }
            Err(err) => {
                debug!(%err, %retry, host = name, "upstream listRepos failed + does not look retryable");
                break;
            }
        }
    }

    debug!(host = name, "listRepos failed or retries exhausted");
    None
}

/// get the complete list of pdses from the upstream relay
async fn list_all_hosts<S: StorageEngine>(
    relay: &Host,
    cancel: &CancellationToken,
) -> Result<Vec<PdsHost>, CrawlError<S::Error>> {
    let mut all = Vec::new();
    let mut cursor = None;
    let mut i = 0;
    loop {
        debug!(?relay, page = i, ?cursor, "listing hosts");
        let Some(maybe) = cancel.run(list_hosts_page(relay, cursor.as_deref())).await else {
            break;
        };
        let Some(resp) = maybe else {
            debug!(?relay, "None response for list_hosts_page");
            break;
        };
        let next = resp.cursor.clone();
        all.extend(resp.hosts);
        match next {
            Some(c) if Some(&c) != cursor.as_ref() => {
                cursor = Some(c);
            }
            _ => {
                info!(?relay, "list_all_hosts finished gathering");
                break;
            }
        }
        i += 1;
    }
    Ok(all)
}

/// get one page from an upstream via com.atproto.sync.listRepos
///
/// calls upstream through `Host` inherit its self-throttling
async fn list_hosts_page(relay: &Host, cursor: Option<&str>) -> Option<ListHostsResponse> {
    let name = relay.name().as_str();

    for retry in 1_u32..=LIST_HOSTS_RETRIES {
        match relay.list_hosts(LIST_HOSTS_LIMIT, cursor).await {
            Ok(r) => return Some(r),
            Err(HostRequestError::HostBackoff { remaining }) => {
                trace!(?remaining, retry, host = name, "host backing off, waiting");
                tokio::time::sleep(remaining).await;
            }
            Err(
                err @ (HostRequestError::RateLimited { .. } | HostRequestError::ServerTransient),
            ) => {
                // (backoff is handled by the `Host` directly)
                trace!(%err, retry, host = name, "backoff response, retrying");
            }
            Err(err @ (HostRequestError::Transport(_) | HostRequestError::Timeout)) => {
                let backoff_secs = (3 + 2_u64.saturating_pow(retry)).min(900);
                trace!(%err, retry, backoff_secs, host = name, "upstream listHosts failed, retrying");
                tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
            }
            Err(err) => {
                debug!(%err, %retry, host = name, "upstream listHosts failed + does not look retryable");
                break;
            }
        }
    }

    debug!(host = name, "listHosts failed or retries exhausted");
    None
}

fn crawlable_hosts_sorted(mut hosts: Vec<PdsHost>) -> Vec<PdsHost> {
    hosts.retain(|h| h.status == Some("active") && h.account_count > 0);
    hosts.sort_by_key(|h| std::cmp::Reverse(h.account_count));
    hosts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pds(hostname: &str, account_count: u64, status: Option<&'static str>) -> PdsHost {
        PdsHost {
            hostname: hostname.to_string(),
            account_count,
            status,
            seq: 0,
        }
    }

    #[test]
    fn crawlable_hosts_drops_inactive_empty_and_sorts_biggest_first() {
        let out = crawlable_hosts_sorted(vec![
            pds("small.active", 3, Some("active")),
            pds("empty.active", 0, Some("active")), // dropped: no accounts
            pds("big.offline", 9_000, Some("offline")), // dropped: not active
            pds("big.active", 900, Some("active")),
            pds("no.status", 5, None), // dropped: no status at all
            pds("mid.active", 50, Some("active")),
        ]);
        let names: Vec<_> = out.iter().map(|h| h.hostname.as_str()).collect();
        // active + non-empty only, ordered by account_count descending
        assert_eq!(names, ["big.active", "mid.active", "small.active"]);
    }
}
