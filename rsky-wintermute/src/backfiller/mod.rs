//! Full-network repo backfill.
//!
//! Replaces the relay-enumerating, unthrottled, queue-backed backfiller. The
//! shape, derived from hubble's own bulk fetcher (`car-dump`) and its sync
//! library's host model:
//!
//! 1. **Discover** PDS hosts from the relay's `listHosts`. Hosts matching the
//!    direct patterns (Bluesky's mushroom fleet by default) are fetched
//!    directly; everything else is left to hubble, the public mirror.
//! 2. **Enumerate** each direct host's `listRepos`, then hubble's. Rows are
//!    keyed by DID with the listed `rev`, so re-enumeration only creates work
//!    for repos that moved. A direct source always owns a repo over hubble.
//! 3. **Fetch** per source, each under its own rate budget and adaptive
//!    concurrency, only while the sink has room. Archives stream to memory or
//!    disk, parse straight to `IndexJob`s, and go into Postgres through the
//!    bulk COPY path -- no intermediate queue.
//! 4. **Settle** each repo once its batch commits: done at the archive's rev,
//!    or retried with a cooldown, or handed to hubble when its own PDS will
//!    not serve it, or written off.
//!
//! Everything here runs alongside the live firehose indexer. The `record.rev`
//! gate in the indexer keeps a stale backfilled record from overwriting a
//! newer live one.

pub mod discovery;
pub mod host;
pub mod hubble;
pub mod pds;
pub mod runner;
pub mod sink;
pub mod source;
pub mod state;

use std::io::Cursor;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use iroh_car::CarReader;
use rsky_repo::parse::get_and_parse_record;
use rsky_repo::readable_repo::ReadableRepo;
use rsky_repo::storage::memory_blockstore::MemoryBlockstore;

use crate::SHUTDOWN;
use crate::types::{IndexJob, WintermuteError, WriteAction};
use host::{HostLimits, HostPolicy, Hostname};
use hubble::HubbleConfig;
use runner::{Runner, RunnerConfig};
use sink::{NullSink, PgSink, PgSinkConfig, RecordSink};
use source::{FetchLimits, RepoBody};
use state::RepoStateStore;

/// One archive, parsed.
#[derive(Debug)]
pub struct ParsedRepo {
    pub did: String,
    /// The commit rev of the archive. This is what gets recorded as indexed.
    pub rev: String,
    pub jobs: Vec<IndexJob>,
    /// Records dropped by the collection allowlist.
    pub filtered: usize,
}

/// Parse a CAR off a reader into index jobs.
///
/// The remaining memory bound is honest: `MemoryBlockstore` materialises every
/// block, so peak is one whole repo. What streaming removes is holding it
/// three times over, and holding an 87 MB body while waiting on a connection.
pub async fn parse_car<R>(did: &str, car_reader: R) -> Result<ParsedRepo, WintermuteError>
where
    R: tokio::io::AsyncRead + Unpin + Send,
{
    let mut reader = CarReader::new(car_reader)
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
    let storage_arc = Arc::new(tokio::sync::RwLock::new(blockstore));

    let mut repo = ReadableRepo::load(storage_arc, root)
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

    let blocks_result = {
        let storage_guard = repo.storage.read().await;
        storage_guard
            .get_blocks(leaves.iter().map(|e| e.value).collect())
            .await
            .map_err(|e| WintermuteError::Repo(format!("get blocks failed: {e}")))?
    };

    let rev = repo.commit.rev.clone();
    let now = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();

    let mut jobs: Vec<IndexJob> = Vec::with_capacity(leaves.len());
    let mut filtered = 0usize;

    for entry in &leaves {
        let Some((collection, rkey)) = entry.key.split_once('/') else {
            continue;
        };
        if !crate::config::ingest_collection_allowed(collection) {
            filtered += 1;
            continue;
        }
        if let Ok(parsed) = get_and_parse_record(&blocks_result.blocks, entry.value) {
            let record_json_raw = serde_json::to_value(&parsed.record)
                .map_err(|e| WintermuteError::Serialization(format!("json failed: {e}")))?;
            let record_json = convert_record_to_ipld(&record_json_raw);
            jobs.push(IndexJob {
                uri: format!("at://{did}/{collection}/{rkey}"),
                cid: entry.value.to_string(),
                action: WriteAction::Create,
                record: Some(record_json),
                indexed_at: now.clone(),
                rev: rev.clone(),
            });
        }
    }

    crate::metrics::BACKFILL_RECORDS_PARSED_TOTAL.inc_by(jobs.len() as u64);
    crate::metrics::BACKFILL_RECORDS_FILTERED_TOTAL.inc_by(filtered as u64);

    Ok(ParsedRepo {
        did: did.to_owned(),
        rev,
        jobs,
        filtered,
    })
}

/// Parse a fetched body, wherever it landed.
pub async fn parse_body(did: &str, body: RepoBody) -> Result<ParsedRepo, WintermuteError> {
    match body {
        RepoBody::Memory(bytes) => parse_car(did, Cursor::new(&bytes[..])).await,
        RepoBody::Spilled { file, .. } => {
            let handle = file
                .reopen()
                .map_err(|e| WintermuteError::Other(format!("spill reopen: {e}")))?;
            parse_car(did, tokio::fs::File::from_std(handle)).await
        }
    }
}

/// Rewrite CBOR byte arrays into their IPLD JSON forms: a CID becomes
/// `{"$link": ...}`, anything else `{"$bytes": base64}`.
pub fn convert_record_to_ipld(record_json: &serde_json::Value) -> serde_json::Value {
    use base64::Engine;

    match record_json {
        serde_json::Value::Object(map) => {
            let mut new_map = serde_json::Map::new();
            for (k, v) in map {
                new_map.insert(k.clone(), convert_record_to_ipld(v));
            }
            serde_json::Value::Object(new_map)
        }
        serde_json::Value::Array(arr) => {
            let is_byte_array = arr.iter().all(|v| {
                matches!(v, serde_json::Value::Number(n) if n.as_u64().is_some_and(|num| num <= 255))
            });

            if is_byte_array && !arr.is_empty() {
                let bytes: Vec<u8> = arr
                    .iter()
                    .filter_map(|v| v.as_u64().and_then(|n| u8::try_from(n).ok()))
                    .collect();

                if let Ok(cid) = lexicon_cid::Cid::try_from(&bytes[..]) {
                    return serde_json::json!({"$link": cid.to_string()});
                }

                let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
                return serde_json::json!({"$bytes": encoded});
            }

            serde_json::Value::Array(arr.iter().map(convert_record_to_ipld).collect())
        }
        other => other.clone(),
    }
}

/// How the backfill is sourced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackfillMode {
    /// Backfill disabled: the manager thread parks until shutdown.
    Off,
    /// Everything through hubble.
    Hubble,
    /// Only hosts matching the direct patterns, nothing through hubble.
    Direct,
    /// Direct hosts directly, everything else through hubble. The default when
    /// enabled.
    Hybrid,
}

impl BackfillMode {
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "off" | "" | "0" | "false" => Some(Self::Off),
            "hubble" => Some(Self::Hubble),
            "direct" | "pds" => Some(Self::Direct),
            "hybrid" | "on" | "true" | "1" => Some(Self::Hybrid),
            _ => None,
        }
    }

    #[must_use]
    pub const fn uses_hubble(self) -> bool {
        matches!(self, Self::Hubble | Self::Hybrid)
    }

    #[must_use]
    pub const fn uses_direct(self) -> bool {
        matches!(self, Self::Direct | Self::Hybrid)
    }
}

/// Where parsed records are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkKind {
    Postgres,
    /// Fetch and parse only. For measuring the fetch side without a database.
    Null,
}

/// Everything the backfill reads from the environment. One place, so the
/// README table and the code cannot drift apart.
#[derive(Debug, Clone)]
pub struct BackfillConfig {
    pub mode: BackfillMode,
    pub sink: SinkKind,
    /// `SQLite` file for per-repo and per-host state.
    pub state_db: PathBuf,
    /// Relay whose `listHosts` discovers PDS hosts, with scheme.
    pub relay_url: Option<String>,
    /// Hosts to fetch directly even if the relay does not list them.
    pub extra_hosts: Vec<Hostname>,
    pub policy: HostPolicy,
    pub hubble: HubbleConfig,
    pub hubble_concurrency: usize,
    /// Concurrent per-source fetch workers (one per host with pending work,
    /// plus hubble).
    pub max_workers: usize,
    pub fetch: FetchLimits,
    pub pg_sink: PgSinkConfig,
    /// Connections for the backfill's own pool.
    pub db_pool_size: usize,
    /// Re-run enumeration this long after the last pass completes. `None`
    /// enumerates once and then only drains.
    pub reenumerate_after: Option<Duration>,
    /// Tokio worker threads for the backfill runtime.
    pub worker_threads: usize,
    pub user_agent: String,
}

fn env_or<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

fn env_list(name: &str) -> Vec<String> {
    std::env::var(name)
        .ok()
        .map(|s| {
            s.split(',')
                .map(|p| p.trim().to_owned())
                .filter(|p| !p.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn nz(v: u32) -> NonZeroU32 {
    NonZeroU32::new(v.max(1)).unwrap_or(NonZeroU32::MIN)
}

impl BackfillConfig {
    /// Read `BACKFILL_*` from the environment. `relay_hosts` are the
    /// `RELAY_HOSTS` the ingester subscribes to; the first is the default
    /// discovery relay.
    #[must_use]
    pub fn from_env(relay_hosts: &[String]) -> Self {
        let mode = std::env::var("BACKFILL_MODE")
            .ok()
            .and_then(|v| BackfillMode::parse(&v))
            .unwrap_or(BackfillMode::Off);
        let sink = match std::env::var("BACKFILL_SINK")
            .unwrap_or_default()
            .to_lowercase()
            .as_str()
        {
            "null" | "none" | "dry-run" => SinkKind::Null,
            _ => SinkKind::Postgres,
        };
        let relay_url = std::env::var("BACKFILL_RELAY")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| relay_hosts.first().cloned())
            .map(|h| {
                if h.starts_with("http://") || h.starts_with("https://") {
                    h
                } else {
                    format!("https://{h}")
                }
            });

        let mut policy = HostPolicy::default();
        let patterns = env_list("BACKFILL_DIRECT_HOSTS");
        if !patterns.is_empty() {
            policy.direct_patterns = patterns;
        }
        policy.bsky = HostLimits {
            rps: nz(env_or("BACKFILL_BSKY_RPS", 10)),
            concurrency: env_or("BACKFILL_BSKY_CONCURRENCY", 10usize).max(1),
            adaptive: false,
        };
        policy.generic = HostLimits {
            rps: nz(env_or("BACKFILL_PDS_RPS", 3)),
            concurrency: env_or("BACKFILL_PDS_CONCURRENCY", 6usize).max(1),
            adaptive: true,
        };
        let extra_hosts: Vec<Hostname> = env_list("BACKFILL_EXTRA_HOSTS")
            .iter()
            .filter_map(|h| Hostname::new(h).ok())
            .collect();
        // Extra hosts are direct by definition.
        for h in &extra_hosts {
            policy.direct_patterns.push(h.as_str().to_owned());
        }

        let user_agent = std::env::var("BACKFILL_USER_AGENT")
            .unwrap_or_else(|_| hubble::DEFAULT_USER_AGENT.to_owned());
        let hubble = HubbleConfig {
            base_url: std::env::var("BACKFILL_HUBBLE_URL")
                .unwrap_or_else(|_| hubble::DEFAULT_BASE_URL.to_owned()),
            user_agent: user_agent.clone(),
            rps: nz(env_or("BACKFILL_HUBBLE_RPS", 8)),
        };

        let writers = env_or("BACKFILL_WRITERS", 4usize).max(1);
        let pg_sink = PgSinkConfig {
            writers,
            batch_jobs: env_or("BACKFILL_BATCH_JOBS", 2000usize).max(1),
            queue_repos: env_or("BACKFILL_QUEUE_REPOS", 256usize).max(1),
            flush_after: Duration::from_millis(env_or("BACKFILL_FLUSH_MS", 500u64)),
            skip_boilerplate: *crate::config::RECORD_SKIP_BOILERPLATE,
        };

        let fetch = FetchLimits {
            spill_threshold: env_or("BACKFILL_SPILL_MB", 1u64) << 20,
            max_body: env_or("BACKFILL_MAX_BODY_MB", 512u64) << 20,
            spill_dir: std::env::var("BACKFILL_SPILL_DIR")
                .map_or_else(|_| std::env::temp_dir(), PathBuf::from),
            request_timeout: Duration::from_secs(env_or("BACKFILL_FETCH_TIMEOUT_SECS", 300)),
        };

        let reenumerate = env_or("BACKFILL_REENUMERATE_SECS", 0u64);

        Self {
            mode,
            sink,
            state_db: std::env::var("BACKFILL_STATE_DB")
                .map_or_else(|_| PathBuf::from("backfill_state.sqlite"), PathBuf::from),
            relay_url,
            extra_hosts,
            policy,
            hubble,
            hubble_concurrency: env_or("BACKFILL_HUBBLE_CONCURRENCY", 4usize).max(1),
            max_workers: env_or("BACKFILL_MAX_WORKERS", 128usize).max(1),
            fetch,
            pg_sink,
            // Each writer holds one connection and fans out to ~6 more inside
            // process_jobs_batch.
            db_pool_size: env_or("BACKFILL_DB_POOL_SIZE", writers * 8).max(writers + 1),
            reenumerate_after: (reenumerate > 0).then(|| Duration::from_secs(reenumerate)),
            worker_threads: env_or("BACKFILL_WORKER_THREADS", num_cpus::get().max(2)),
            user_agent,
        }
    }

    #[must_use]
    pub fn runner_config(&self) -> RunnerConfig {
        RunnerConfig {
            hubble: self.mode.uses_hubble().then(|| self.hubble.clone()),
            direct: self.mode.uses_direct(),
            relay_url: self.relay_url.clone(),
            extra_hosts: self.extra_hosts.clone(),
            policy: self.policy.clone(),
            hubble_concurrency: self.hubble_concurrency,
            max_workers: self.max_workers,
            fetch: self.fetch.clone(),
            reenumerate_after: self.reenumerate_after,
            user_agent: self.user_agent.clone(),
            ..RunnerConfig::default()
        }
    }
}

/// The backfill thread of the `wintermute` binary.
pub struct BackfillManager {
    cfg: BackfillConfig,
    database_url: String,
}

impl BackfillManager {
    #[must_use]
    pub const fn new(cfg: BackfillConfig, database_url: String) -> Self {
        Self { cfg, database_url }
    }

    pub fn run(self) -> Result<(), WintermuteError> {
        if self.cfg.mode == BackfillMode::Off {
            tracing::info!("backfill disabled (BACKFILL_MODE=off); nothing to do");
            while !SHUTDOWN.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_secs(5));
            }
            return Ok(());
        }

        tracing::info!(
            mode = ?self.cfg.mode,
            sink = ?self.cfg.sink,
            state_db = %self.cfg.state_db.display(),
            relay = ?self.cfg.relay_url,
            direct = ?self.cfg.policy.direct_patterns,
            hubble = %self.cfg.hubble.base_url,
            "backfill starting"
        );

        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(self.cfg.worker_threads)
            .thread_name("wintermute-backfill")
            .enable_all()
            .build()
            .map_err(|e| WintermuteError::Other(format!("failed to create runtime: {e}")))?;

        let state = RepoStateStore::open(&self.cfg.state_db)
            .map_err(|e| WintermuteError::Other(format!("backfill state: {e}")))?;

        rt.block_on(async {
            match self.cfg.sink {
                SinkKind::Postgres => {
                    let pool = crate::config::create_pg_pool(
                        &self.database_url,
                        crate::config::pg_pool_config(self.cfg.db_pool_size),
                    )?;
                    crate::metrics::register_pool("backfill", &pool);
                    let sink = Arc::new(PgSink::new(&pool, &self.cfg.pg_sink));
                    let runner = Arc::new(Runner::new(
                        self.cfg.runner_config(),
                        Arc::clone(&sink),
                        state,
                    )?);
                    let result = runner.run().await;
                    if let Some(sink) = Arc::into_inner(sink) {
                        sink.finish().await;
                    }
                    result
                }
                SinkKind::Null => {
                    let sink = Arc::new(NullSink::default());
                    let runner = Arc::new(Runner::new(
                        self.cfg.runner_config(),
                        Arc::clone(&sink),
                        state,
                    )?);
                    let result = runner.run().await;
                    tracing::info!(
                        repos = sink.repos.load(Ordering::Relaxed),
                        records = sink.records.load(Ordering::Relaxed),
                        "dry-run sink totals"
                    );
                    result
                }
            }
        })
        .map_err(|e| WintermuteError::Other(format!("backfill: {e}")))
    }
}

/// Sample state-store gauges. Called by the runner's progress ticker.
pub fn sample_state_metrics(state: &RepoStateStore) {
    if let Ok(stats) = state.stats() {
        let g = &crate::metrics::BACKFILL_REPOS_BY_STATE;
        g.with_label_values(&["pending"])
            .set(i64::try_from(stats.pending).unwrap_or(i64::MAX));
        g.with_label_values(&["claimed"])
            .set(i64::try_from(stats.claimed).unwrap_or(i64::MAX));
        g.with_label_values(&["done"])
            .set(i64::try_from(stats.done).unwrap_or(i64::MAX));
        g.with_label_values(&["terminal"])
            .set(i64::try_from(stats.terminal).unwrap_or(i64::MAX));
    }
}

/// Whether `sink` reports room; shared by the runner and tests.
pub fn sink_has_room<K: RecordSink>(sink: &K) -> bool {
    sink.has_capacity()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_parse_leniently_and_map_to_sources() {
        assert_eq!(BackfillMode::parse("off"), Some(BackfillMode::Off));
        assert_eq!(BackfillMode::parse(""), Some(BackfillMode::Off));
        assert_eq!(BackfillMode::parse("HUBBLE"), Some(BackfillMode::Hubble));
        assert_eq!(BackfillMode::parse("pds"), Some(BackfillMode::Direct));
        assert_eq!(BackfillMode::parse(" hybrid "), Some(BackfillMode::Hybrid));
        assert_eq!(BackfillMode::parse("true"), Some(BackfillMode::Hybrid));
        assert_eq!(BackfillMode::parse("banana"), None);

        assert!(BackfillMode::Hybrid.uses_hubble() && BackfillMode::Hybrid.uses_direct());
        assert!(BackfillMode::Hubble.uses_hubble() && !BackfillMode::Hubble.uses_direct());
        assert!(!BackfillMode::Direct.uses_hubble() && BackfillMode::Direct.uses_direct());
        assert!(!BackfillMode::Off.uses_hubble() && !BackfillMode::Off.uses_direct());
    }

    #[test]
    fn config_from_env_defaults() {
        // Env is process-global; only read keys nothing else in the test
        // binary sets.
        let cfg = BackfillConfig::from_env(&["bsky.network".to_owned()]);
        assert_eq!(cfg.relay_url.as_deref(), Some("https://bsky.network"));
        assert!(
            cfg.policy
                .is_direct(&Hostname::new("x.host.bsky.network").unwrap())
        );
        assert!(cfg.hubble_concurrency >= 1);
        assert!(cfg.db_pool_size > cfg.pg_sink.writers);
        assert!(cfg.user_agent.contains('@'));
        let rc = cfg.runner_config();
        assert_eq!(rc.hubble.is_some(), cfg.mode.uses_hubble());
        assert_eq!(rc.direct, cfg.mode.uses_direct());

        let cfg = BackfillConfig::from_env(&["https://relay.example/".to_owned()]);
        assert_eq!(cfg.relay_url.as_deref(), Some("https://relay.example/"));
        let cfg = BackfillConfig::from_env(&[]);
        assert_eq!(cfg.relay_url, None);
    }

    #[test]
    fn env_helpers() {
        assert_eq!(env_or("BACKFILL_TEST_UNSET_KEY_XYZ", 7u32), 7);
        assert!(env_list("BACKFILL_TEST_UNSET_KEY_XYZ").is_empty());
        assert_eq!(nz(0).get(), 1);
        assert_eq!(nz(5).get(), 5);
    }

    #[test]
    fn convert_record_to_ipld_rewrites_bytes_and_cids() {
        // A dag-cbor CID as a byte array: version 1, dag-cbor, sha2-256, 32 bytes.
        let mut cid_bytes = vec![0x01u8, 0x71, 0x12, 0x20];
        cid_bytes.extend(std::iter::repeat_n(0xabu8, 32));
        let raw = serde_json::json!({
            "text": "hi",
            "ref": cid_bytes,
            "blob": [1, 2, 3],
            "nums": [1, 2, 300],
            "nested": {"inner": [0, 255]},
            "empty": [],
        });
        let out = convert_record_to_ipld(&raw);
        assert_eq!(out["text"], "hi");
        assert!(out["ref"]["$link"].as_str().unwrap().starts_with("bafyrei"));
        assert_eq!(out["blob"]["$bytes"], "AQID");
        assert_eq!(out["nums"], serde_json::json!([1, 2, 300]), "not bytes");
        assert_eq!(out["nested"]["inner"]["$bytes"], "AP8=");
        assert_eq!(out["empty"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn garbage_is_not_a_car() {
        let err = parse_car("did:plc:x", Cursor::new(&b"nope"[..]))
            .await
            .unwrap_err();
        assert!(matches!(err, WintermuteError::Repo(_)));
        let err = parse_body("did:plc:x", RepoBody::Memory(b"nope".to_vec()))
            .await
            .unwrap_err();
        assert!(matches!(err, WintermuteError::Repo(_)));
    }

    #[test]
    fn off_mode_manager_returns_on_shutdown() {
        let cfg = BackfillConfig {
            mode: BackfillMode::Off,
            ..BackfillConfig::from_env(&[])
        };
        SHUTDOWN.store(true, Ordering::Relaxed);
        let r = BackfillManager::new(cfg, "postgres://x".into()).run();
        SHUTDOWN.store(false, Ordering::Relaxed);
        assert!(r.is_ok());
    }
}
