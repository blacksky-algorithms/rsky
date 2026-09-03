//! Prototype driver for hubble-sourced backfill.
//!
//! Standalone so the new path can be enumerated, drained and measured against
//! the real service without touching a running indexer. `probe` in particular
//! exercises fetch -> parse -> enqueue end to end against a scratch store, so
//! it proves the whole path with nothing at stake.
//!
//!   hubble_backfill probe --did did:plc:...      # one repo, scratch store
//!   hubble_backfill enumerate --max-pages 5      # walk listRepos into state
//!   hubble_backfill status                       # counts by state
//!   hubble_backfill drain --max 200              # fetch + index, demand-gated
//!   hubble_backfill run                          # enumerate, then drain forever

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use color_eyre::Result;
use color_eyre::eyre::eyre;

use rsky_wintermute::backfiller::hubble::{
    DEFAULT_BASE_URL, DEFAULT_USER_AGENT, HubbleConfig, HubbleSource,
};
use rsky_wintermute::backfiller::runner::{RecordSink, Runner, RunnerConfig, StorageSink};
use rsky_wintermute::backfiller::source::{RepoBody, RepoSource};
use rsky_wintermute::backfiller::state::RepoStateStore;
use rsky_wintermute::storage::Storage;

#[derive(Debug, Parser)]
#[command(
    name = "hubble_backfill",
    about = "hubble-sourced repo backfill (prototype)"
)]
struct Args {
    /// Where per-repo backfill state lives.
    #[arg(
        long,
        env = "BACKFILL_STATE_DB",
        default_value = "hubble-backfill.sqlite"
    )]
    state_db: PathBuf,

    /// wintermute store directory. Its `firehose_backfill` queue is both the
    /// destination and the demand signal.
    #[arg(long, env = "BACKFILL_CACHE_DIR", default_value = "backfill_cache")]
    db_path: PathBuf,

    #[arg(long, env = "HUBBLE_BASE_URL", default_value = DEFAULT_BASE_URL)]
    base_url: String,

    /// hubble requires a user-agent identifying the app with a contact address.
    #[arg(long, env = "HUBBLE_USER_AGENT", default_value = DEFAULT_USER_AGENT)]
    user_agent: String,

    /// Self-imposed request rate. Measured capacity is ~119 repos/s; this is
    /// deliberately a small fraction of it.
    #[arg(long, default_value_t = 8)]
    rps: u32,

    /// In-flight fetches.
    #[arg(long, default_value_t = 4)]
    concurrency: usize,

    /// Stop claiming work when the indexer's backlog reaches this.
    #[arg(long, default_value_t = 200_000)]
    high_watermark: usize,

    /// Resume claiming once it falls back to this.
    #[arg(long, default_value_t = 50_000)]
    low_watermark: usize,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Fetch one repo into a scratch store and report what came back.
    /// Touches no real state; proves fetch -> parse -> enqueue.
    Probe {
        #[arg(long)]
        did: String,
    },
    /// Walk listRepos into keyed state. Resumable, idempotent.
    Enumerate {
        /// Stop after this many pages (each up to 1000 repos).
        #[arg(long)]
        max_pages: Option<u64>,
    },
    /// Counts by state.
    Status,
    /// Fetch and index claimed repos, gated on indexer backlog.
    Drain {
        #[arg(long)]
        max: Option<u64>,
    },
    /// Enumerate, then drain until stopped.
    Run,
}

fn hubble(args: &Args) -> Result<HubbleSource> {
    let cfg = HubbleConfig {
        base_url: args.base_url.clone(),
        user_agent: args.user_agent.clone(),
        rps: NonZeroU32::new(args.rps).ok_or_else(|| eyre!("--rps must be non-zero"))?,
        ..HubbleConfig::default()
    };
    HubbleSource::new(cfg).map_err(|e| eyre!("hubble source: {e}"))
}

fn runner_cfg(args: &Args) -> RunnerConfig {
    RunnerConfig {
        concurrency: args.concurrency,
        queue_high_watermark: args.high_watermark,
        queue_low_watermark: args.low_watermark,
        ..RunnerConfig::default()
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,rsky_wintermute=debug".into()),
        )
        .init();

    let args = Args::parse();

    match &args.cmd {
        Cmd::Probe { did } => probe(&args, did).await,
        Cmd::Enumerate { max_pages } => enumerate(&args, *max_pages).await,
        Cmd::Status => status(&args),
        Cmd::Drain { max } => drain(&args, *max).await,
        Cmd::Run => run(&args).await,
    }
}

/// End-to-end smoke test against the live service, into a throwaway store.
async fn probe(args: &Args, did: &str) -> Result<()> {
    let source = hubble(args)?;
    let scratch = tempfile::tempdir()?;
    let storage = Arc::new(
        Storage::new(Some(scratch.path().join("store")))
            .map_err(|e| eyre!("scratch storage: {e}"))?,
    );
    let sink = StorageSink::new(Arc::clone(&storage));

    let started = std::time::Instant::now();
    let body = source
        .fetch_repo(
            did,
            HubbleConfig::default().spill_threshold,
            HubbleConfig::default().max_body,
        )
        .await
        .map_err(|e| eyre!("fetch {did}: {e}"))?;
    let fetched_ms = started.elapsed().as_millis();
    let (bytes, spilled) = (body.len(), body.spilled());

    let parse_started = std::time::Instant::now();
    let records = sink
        .ingest(did, body, false)
        .await
        .map_err(|e| eyre!("parse/enqueue {did}: {e}"))?;
    let parse_ms = parse_started.elapsed().as_millis();

    // Cheap metadata call, to show what triage without a transfer looks like.
    let info = source.repo_info(did).await.ok();

    println!("did            {did}");
    println!(
        "bytes          {bytes} ({})",
        if spilled {
            "spilled to disk"
        } else {
            "in memory"
        }
    );
    println!("fetch          {fetched_ms} ms");
    println!("parse+enqueue  {parse_ms} ms");
    println!("records        {records} enqueued after the collection allowlist");
    println!("queue depth    {}", sink.depth());
    if let Some(info) = info {
        println!(
            "getRepoInfo    records={:?} rev={:?} state={:?}",
            info.archive.as_ref().and_then(|a| a.records),
            info.sync_state.as_ref().and_then(|s| s.rev.clone()),
            info.sync_state.as_ref().and_then(|s| s.state.clone()),
        );
    }
    Ok(())
}

async fn enumerate(args: &Args, max_pages: Option<u64>) -> Result<()> {
    let state = RepoStateStore::open(&args.state_db).map_err(|e| eyre!("state: {e}"))?;
    let source = hubble(args)?;
    let mut cfg = runner_cfg(args);
    // Enumeration never fetches, so the gate is irrelevant here.
    cfg.queue_high_watermark = usize::MAX;

    let runner = Runner::new(source, NullSink, state.clone(), cfg);
    let report = runner
        .enumerate_pages(max_pages)
        .await
        .map_err(|e| eyre!("{e}"))?;
    println!("{report:#?}");
    println!("{:#?}", state.stats().map_err(|e| eyre!("{e}"))?);
    Ok(())
}

fn status(args: &Args) -> Result<()> {
    let state = RepoStateStore::open(&args.state_db).map_err(|e| eyre!("state: {e}"))?;
    let stats = state.stats().map_err(|e| eyre!("{e}"))?;
    println!("pending   {}", stats.pending);
    println!("claimed   {}", stats.claimed);
    println!("done      {}", stats.done);
    println!("terminal  {}", stats.terminal);
    println!("total     {}", stats.total());
    if let Ok(Some(c)) = state.get_cursor("backfill_enum:hubble") {
        println!("cursor    {c}");
    } else {
        println!("cursor    (none -- next pass starts from the beginning)");
    }
    Ok(())
}

async fn drain(args: &Args, max: Option<u64>) -> Result<()> {
    let state = RepoStateStore::open(&args.state_db).map_err(|e| eyre!("state: {e}"))?;
    let storage =
        Arc::new(Storage::new(Some(args.db_path.clone())).map_err(|e| eyre!("storage: {e}"))?);
    let runner = Runner::new(
        hubble(args)?,
        StorageSink::new(storage),
        state.clone(),
        runner_cfg(args),
    );
    let report = runner.drain(max).await.map_err(|e| eyre!("{e}"))?;
    println!("{report:#?}");
    println!("{:#?}", state.stats().map_err(|e| eyre!("{e}"))?);
    Ok(())
}

async fn run(args: &Args) -> Result<()> {
    let state = RepoStateStore::open(&args.state_db).map_err(|e| eyre!("state: {e}"))?;
    let storage =
        Arc::new(Storage::new(Some(args.db_path.clone())).map_err(|e| eyre!("storage: {e}"))?);
    let runner = Runner::new(
        hubble(args)?,
        StorageSink::new(storage),
        state,
        runner_cfg(args),
    );
    let enumerated = runner.enumerate().await.map_err(|e| eyre!("{e}"))?;
    tracing::info!(?enumerated, "enumeration finished, draining");
    let report = runner.drain(None).await.map_err(|e| eyre!("{e}"))?;
    println!("{report:#?}");
    Ok(())
}

/// Sink for enumeration-only commands, which never ingest.
struct NullSink;

impl RecordSink for NullSink {
    fn depth(&self) -> usize {
        0
    }
    async fn ingest(
        &self,
        _did: &str,
        _body: RepoBody,
        _priority: bool,
    ) -> Result<usize, rsky_wintermute::types::WintermuteError> {
        Ok(0)
    }
}
