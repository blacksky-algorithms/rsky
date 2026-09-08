//! sync setup

use std::fmt;
use std::num::{NonZeroU32, NonZeroUsize};
use std::path::PathBuf;
use std::time::Duration;

use crate::firehose::FirehoseConfig;
use crate::host::{Host, HostRegistryConfig};

#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// max repo actors
    ///
    /// default: 32,768
    ///
    /// tune down to save memory at the cost of more i/o
    pub max_repo_actors: usize,
    /// max resync concurrency
    ///
    /// default 1024
    pub max_resync_concurrency: usize,
    /// how many tasks can be queued inside a repo actor before we desync
    ///
    /// note: not every message to the actor results in a queue entry -- tasks
    /// may be dropped, coallesced, or injected by the actor intake.
    ///
    /// default: 128,
    ///
    /// tune down to save memory at the cost of more repo desyncs
    ///
    /// TODO: note which metric (todo) indicates that this is too small
    pub repo_tasks_limit: usize,
    /// repo task intake queue capacity
    ///
    /// default: 4
    ///
    /// this is an advanced setting. if the intake queue is filling up, it
    /// likely points to other problems (ie., actor tasks getting starved)
    pub repo_intake_capacity: usize,
    /// global limit on how many large repos can be resyncing at the same time
    ///
    /// default: 4
    pub big_repo_resync_limit: usize,
    /// car-block-bytes until a repo is classified as "big", in MiB
    ///
    /// note that currently we only count block bytes, not the total download
    /// stream, so this doesn't line up exactly 1:1 with CAR size, but it's not
    /// far off.
    ///
    /// TODO: this config isn't live yet, it needs to be threaded through
    ///
    /// default: 4
    pub big_repo_threshold_mb: usize,
    /// location to keep disk-spilled repo buffers for resync
    ///
    /// default: ./local/
    pub huge_repo_spill_dir: PathBuf,
    /// global limit on resync dispatches
    ///
    /// resync messages are are usually backpressured by checking the host's
    /// available concurrency slots before dispatching, but since these are not
    /// claimed synchronously by actors, there can be racing periods where we
    /// might dispatch too many resync messages.
    ///
    /// this top-level limit at least bounds how quickly we can do that!
    ///
    /// default: 10_000
    pub resync_dispatch_qps: NonZeroU32,
    /// plc identity directory
    ///
    /// default: https://plc.directory
    ///
    /// TODO: typed url
    pub plc_url: String, // TODO typed url
    /// limit on identity-pending repos queued in memory from backfill
    ///
    /// the limit is not strict: identity-pending repos discovered from the
    /// firehose do not observe this, and there may be momentary overshoots when
    /// a batch is added after waiting for space.
    ///
    /// additionally, the limit is only computed against entries that are **due
    /// now** -- requeued future retries will add overshoot. hopefully we don't
    /// usually have too many retries queued.
    ///
    /// (we could, and maybe should, put a second harder limit inclusive of
    /// retries, since a plc outage during backfill might let that run big)
    ///
    /// default: 32,768
    pub pending_identity_queue_limit: NonZeroUsize,
    /// concurrency limit on identity resolution from the scheduled queue
    ///
    /// this is the main way backpressure gets back to backfill strategies
    ///
    /// default: 3 (i just made up a number! probably measure something for this!)
    pub scheduled_resolve_limit: NonZeroUsize,
    /// how long a resync will wait for a permit if it didn't know it needed one
    ///
    /// *preacquired* permits wait as long as needed, since they don't hold open
    /// any resources.
    ///
    /// default: 10s
    pub reactive_permit_wait_timeout: Duration,
    /// key prefix for hubble-sync's state, in every CF/keyspace provided
    ///
    /// must be exclusive: your app must never write keys under this prefix. you
    /// can use `PrefixedEngine` to make sure all your keys are under a
    /// different prefix, or use separate column families/keyspaces entirely, or
    /// just use care not to trample.
    ///
    /// default: `[0x00]`
    pub hubble_sync_storage_prefix: &'static [u8],
    /// policy for which repos get synchronized
    ///
    /// default: SyncScope::Everything
    pub sync_scope: SyncScope,
    /// start a task to route a `ReconcileScope` message to every
    /// [`SyncStatus::OutOfScope`] repo.
    ///
    /// one-shot at startup.
    ///
    /// default: false
    pub rescope_sweep_on_start: bool,
    /// upstream (subscribeRepos source) config
    pub upstream: UpstreamConfig,
    /// deep crawl config
    pub deep_crawl: Option<DeepCrawl>,
    /// firehose subscriber tuning
    pub firehose: FirehoseConfig,
    /// host configs
    pub hosts: HostRegistryConfig,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            max_repo_actors: 32_768,
            max_resync_concurrency: 1024,
            repo_tasks_limit: 128,
            repo_intake_capacity: 4,
            big_repo_resync_limit: 4,
            big_repo_threshold_mb: 4,
            huge_repo_spill_dir: PathBuf::from("./local/"),
            resync_dispatch_qps: NonZeroU32::new(10_000).expect("nonzero literal"),
            plc_url: "https://plc.directory".to_string(),
            pending_identity_queue_limit: 32_768.try_into().unwrap(),
            scheduled_resolve_limit: 3.try_into().unwrap(),
            reactive_permit_wait_timeout: Duration::from_secs(10),
            hubble_sync_storage_prefix: &[0x00],
            sync_scope: SyncScope::default(),
            rescope_sweep_on_start: false,
            upstream: UpstreamConfig::default(),
            deep_crawl: None,
            firehose: FirehoseConfig::default(),
            hosts: HostRegistryConfig::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpstreamConfig {
    pub kind: UpstreamKind,
    /// `com.atproto.sync.subscribeRepos` firehose source
    ///
    /// can be a relay or an individual PDS (multi-relay, one day)
    ///
    /// default: bsky.network
    pub hostname: String, // TODO typed url + hostname normalized? host-interned even?
    pub host_qps: Option<NonZeroU32>,
    pub host_concurrency: Option<usize>,
    /// bearer token for `com.atproto.sync.getRepo` requests to this upstream
    ///
    /// panics: if this is set with a relay-kind upstream. it's only allowed
    /// with PDS direct upstreams.
    ///
    /// default: none
    pub get_repo_token: Option<SensitiveToken>,
    /// hostnames the Authorization header may follow cross-origin redirects to
    ///
    /// normally auth is dropped whenever a redirect leaves the origin
    /// (standard client behaviour). some upstreams (bridgy) serve
    /// subscribeRepos on the PDS hostname but 301 authed getRepo over to a
    /// sibling subdomain -- allowlist that redirect target here to let the
    /// token travel with the hop.
    ///
    /// panics: at host registry setup, if an entry isn't a valid bare hostname
    ///
    /// default: empty (never forward)
    pub get_repo_token_forward_to: Vec<String>,
}

impl Default for UpstreamConfig {
    fn default() -> Self {
        Self {
            hostname: "morel.us-east.host.bsky.network".to_string(), // temp
            kind: UpstreamKind::PdsDirect,                           // temp
            host_qps: None,
            host_concurrency: None,
            get_repo_token: None,
            get_repo_token_forward_to: Vec::new(),
        }
    }
}

/// a sensitive value that redacts itself from `Debug` output
#[derive(Clone)]
pub struct SensitiveToken(String);

impl SensitiveToken {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for SensitiveToken {
    fn from(token: String) -> Self {
        Self(token)
    }
}

impl fmt::Debug for SensitiveToken {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("SensitiveToken(<redacted>)")
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamKind {
    PdsDirect,
    #[default]
    Relay,
}

impl UpstreamKind {
    pub fn is_relay(&self) -> bool {
        matches!(self, UpstreamKind::Relay)
    }
}

/// policy for repo tracking
///
/// right now hacked for one non-everything policy, but later this will be
/// more consumer-app-configurable (and work together with discovery strategies)
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SyncScope {
    /// track only bsky-pds repos
    ///
    /// bluesky has reasonable rate-limiting and other defensive config; many
    /// indie pdses run their machines close to the margin and without much
    /// defense. so, it can be useful to run non-permanent tests only against
    /// bluesky (in particular because it's still *nearly* full-network alone).
    BskyPdsOnly,
    /// track everything
    #[default]
    Everything,
}

impl SyncScope {
    pub fn allows_host(&self, host: &Host) -> bool {
        match self {
            SyncScope::BskyPdsOnly => host.name().is_bsky(),
            SyncScope::Everything => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeepCrawl {
    pub concurrency: NonZeroUsize,
    pub recrawl_interval: Duration,
    pub wait_for_upstream_crawl: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_token_debug_is_redacted() {
        // SyncConfig derives Debug, so a stray `?config` in a tracing call
        // must never print the secret
        let token = SensitiveToken::from("sekrit".to_string());
        assert!(!format!("{token:?}").contains("sekrit"));
        let config = UpstreamConfig {
            get_repo_token: Some(token),
            ..Default::default()
        };
        assert!(!format!("{config:?}").contains("sekrit"));
    }
}
