//! Standalone driver for the repo backfill.
//!
//! The same machinery `wintermute` runs on its backfill thread, exposed as
//! subcommands so each stage can be run, inspected and measured on its own:
//!
//!   backfill discover                  # relay listHosts -> host table
//!   backfill enumerate [--max-pages N] # direct hosts, then hubble, into state
//!   backfill status                    # counts by state and by source
//!   backfill drain                     # fetch + index until idle (demand-gated)
//!   backfill run                       # enumerate and drain, like the daemon
//!   backfill probe --did did:plc:...   # fetch + parse one repo, write nothing
//!
//! Configuration is the `BACKFILL_*` environment (see the README), plus
//! `DATABASE_URL` for anything that writes. `BACKFILL_SINK=null` fetches and
//! parses without a database, which is how fetch-side throughput is measured.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use clap::{Parser, Subcommand};
use color_eyre::Result;
use color_eyre::eyre::eyre;

use rsky_wintermute::backfiller::host::Hostname;
use rsky_wintermute::backfiller::runner::Runner;
use rsky_wintermute::backfiller::sink::{NullSink, PgSink, RecordSink};
use rsky_wintermute::backfiller::source::RepoSource;
use rsky_wintermute::backfiller::state::RepoStateStore;
use rsky_wintermute::backfiller::{BackfillConfig, BackfillMode, SinkKind};

#[derive(Debug, Parser)]
#[command(name = "backfill", about = "repo backfill: discover, enumerate, drain")]
struct Args {
    /// Relay hosts, as the daemon would get them; the first is the discovery
    /// relay unless BACKFILL_RELAY is set.
    #[arg(
        long,
        env = "RELAY_HOSTS",
        value_delimiter = ',',
        default_value = "bsky.network"
    )]
    relay_hosts: Vec<String>,

    #[arg(long, env = "DATABASE_URL")]
    database_url: Option<String>,

    /// Override BACKFILL_MODE (off|hubble|direct|hybrid).
    #[arg(long)]
    mode: Option<String>,

    /// Serve /metrics on this port while running.
    #[arg(long, env = "METRICS_PORT")]
    metrics_port: Option<u16>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Walk the relay's listHosts into the host table.
    Discover,
    /// Enumerate direct hosts, then hubble. Resumable, idempotent.
    Enumerate {
        /// Stop each source after this many pages (each up to 1000 repos).
        #[arg(long)]
        max_pages: Option<u64>,
        /// Only this source (a hostname, or `hubble`).
        #[arg(long)]
        source: Option<String>,
    },
    /// Counts by state and by source.
    Status,
    /// Fetch and index everything pending, then exit.
    Drain,
    /// Enumerate, then drain; re-enumerate if BACKFILL_REENUMERATE_SECS is set.
    Run,
    /// Fetch one repo and parse it, writing nothing.
    Probe {
        #[arg(long)]
        did: String,
        /// Fetch from this PDS host instead of hubble.
        #[arg(long)]
        host: Option<String>,
    },
}

fn config(args: &Args) -> Result<BackfillConfig> {
    let mut cfg = BackfillConfig::from_env(&args.relay_hosts);
    if let Some(m) = &args.mode {
        cfg.mode = BackfillMode::parse(m).ok_or_else(|| eyre!("bad --mode {m}"))?;
    }
    if cfg.mode == BackfillMode::Off {
        // The CLI is an explicit request; default to the full shape.
        cfg.mode = BackfillMode::Hybrid;
    }
    Ok(cfg)
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,rsky_wintermute=info".into()),
        )
        .init();
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .map_err(|_| eyre!("failed to install rustls crypto provider"))?;

    let args = Args::parse();
    let cfg = config(&args)?;
    rsky_wintermute::metrics::initialize_metrics();
    if let Some(port) = args.metrics_port {
        std::thread::spawn(move || {
            if let Err(e) = rsky_wintermute::metrics_server::serve(port) {
                tracing::error!("metrics server: {e}");
            }
        });
    }

    let state = RepoStateStore::open(&cfg.state_db).map_err(|e| eyre!("state: {e}"))?;
    tracing::info!(mode = ?cfg.mode, state_db = %cfg.state_db.display(), "backfill cli");

    match &args.cmd {
        Cmd::Status => status(&state),
        Cmd::Probe { did, host } => probe(&cfg, &state, did, host.as_deref()).await,
        Cmd::Discover => {
            let r = Arc::new(runner_null(&cfg, state.clone())?);
            let n = r.discover().await.map_err(|e| eyre!("{e}"))?;
            println!("direct hosts: {n}");
            for h in state.direct_hosts().map_err(|e| eyre!("{e}"))? {
                println!(
                    "  {:<45} accounts={:<9} status={:<8} list={:?}",
                    h.host,
                    h.account_count,
                    h.status.as_deref().unwrap_or("-"),
                    h.list_state
                );
            }
            Ok(())
        }
        Cmd::Enumerate { max_pages, source } => {
            let r = Arc::new(runner_null(&cfg, state.clone())?);
            match source {
                Some(s) if s == "hubble" => {
                    let h = r
                        .hubble()
                        .ok_or_else(|| eyre!("hubble not enabled"))?
                        .clone();
                    let rep = r
                        .enumerate_pages(h.as_ref(), *max_pages)
                        .await
                        .map_err(|e| eyre!("{e}"))?;
                    println!("{rep:#?}");
                }
                Some(s) => {
                    let host = Hostname::new(s).map_err(|e| eyre!("{e}"))?;
                    let src = r.pds_source(&host);
                    let rep = r
                        .enumerate_pages(&src, *max_pages)
                        .await
                        .map_err(|e| eyre!("{e}"))?;
                    println!("{rep:#?}");
                }
                None if max_pages.is_some() => {
                    r.discover().await.map_err(|e| eyre!("{e}"))?;
                    for row in state.direct_hosts().map_err(|e| eyre!("{e}"))? {
                        let src = r.pds_source(&row.host);
                        let rep = r
                            .enumerate_pages(&src, *max_pages)
                            .await
                            .map_err(|e| eyre!("{e}"))?;
                        println!("{:<45} {rep:?}", row.host);
                    }
                    if let Some(h) = r.hubble() {
                        let rep = r
                            .enumerate_pages(h.as_ref(), *max_pages)
                            .await
                            .map_err(|e| eyre!("{e}"))?;
                        println!("{:<45} {rep:?}", "hubble");
                    }
                }
                None => {
                    r.enumerate_all().await.map_err(|e| eyre!("{e}"))?;
                }
            }
            status(&state)
        }
        Cmd::Drain => match cfg.sink {
            SinkKind::Null => {
                let r = Arc::new(runner_null(&cfg, state.clone())?);
                r.progress.enumeration_done.store(true, Ordering::Relaxed);
                r.drain(true).await.map_err(|e| eyre!("{e}"))?;
                status(&state)
            }
            SinkKind::Postgres => {
                let (r, sink) = runner_pg(&args, &cfg, state.clone())?;
                r.progress.enumeration_done.store(true, Ordering::Relaxed);
                let result = r.drain(true).await;
                drop(r);
                if let Some(sink) = Arc::into_inner(sink) {
                    sink.finish().await;
                }
                result.map_err(|e| eyre!("{e}"))?;
                status(&state)
            }
        },
        Cmd::Run => match cfg.sink {
            SinkKind::Null => {
                let r = Arc::new(runner_null(&cfg, state.clone())?);
                r.run().await.map_err(|e| eyre!("{e}"))?;
                status(&state)
            }
            SinkKind::Postgres => {
                let (r, sink) = runner_pg(&args, &cfg, state.clone())?;
                let result = r.run().await;
                if let Some(sink) = Arc::into_inner(sink) {
                    sink.finish().await;
                }
                result.map_err(|e| eyre!("{e}"))?;
                status(&state)
            }
        },
    }
}

fn runner_null(cfg: &BackfillConfig, state: RepoStateStore) -> Result<Runner<NullSink>> {
    Runner::new(cfg.runner_config(), Arc::new(NullSink::default()), state).map_err(|e| eyre!("{e}"))
}

fn runner_pg(
    args: &Args,
    cfg: &BackfillConfig,
    state: RepoStateStore,
) -> Result<(Arc<Runner<PgSink>>, Arc<PgSink>)> {
    let url = args
        .database_url
        .as_deref()
        .ok_or_else(|| eyre!("DATABASE_URL is required unless BACKFILL_SINK=null"))?;
    let pool = rsky_wintermute::config::create_pg_pool(
        url,
        rsky_wintermute::config::pg_pool_config(cfg.db_pool_size),
    )
    .map_err(|e| eyre!("{e}"))?;
    rsky_wintermute::metrics::register_pool("backfill", &pool);
    let sink = Arc::new(PgSink::new(&pool, &cfg.pg_sink));
    let runner =
        Runner::new(cfg.runner_config(), Arc::clone(&sink), state).map_err(|e| eyre!("{e}"))?;
    Ok((Arc::new(runner), sink))
}

fn status(state: &RepoStateStore) -> Result<()> {
    let stats = state.stats().map_err(|e| eyre!("{e}"))?;
    println!("pending   {}", stats.pending);
    println!("claimed   {}", stats.claimed);
    println!("done      {}", stats.done);
    println!("terminal  {}", stats.terminal);
    println!("total     {}", stats.total());
    let by = state.stats_by_source().map_err(|e| eyre!("{e}"))?;
    if !by.is_empty() {
        println!(
            "\n{:<45} {:>10} {:>10} {:>10}",
            "source", "pending", "done", "terminal"
        );
        for (source, p, d, t) in by.iter().take(40) {
            println!("{source:<45} {p:>10} {d:>10} {t:>10}");
        }
        if by.len() > 40 {
            println!("... and {} more sources", by.len() - 40);
        }
    }
    if let Ok(Some(c)) = state.get_cursor("enum:hubble") {
        println!("\nhubble cursor  {c}");
    }
    Ok(())
}

/// Fetch one repo, parse it, and say what came back. Writes nothing.
async fn probe(
    cfg: &BackfillConfig,
    state: &RepoStateStore,
    did: &str,
    host: Option<&str>,
) -> Result<()> {
    let r = runner_null(cfg, state.clone())?;
    let sink = NullSink::default();
    let started = std::time::Instant::now();
    let body = match host {
        Some(h) => {
            let host = Hostname::new(h).map_err(|e| eyre!("{e}"))?;
            r.pds_source(&host)
                .fetch_repo(did.to_owned(), cfg.fetch.clone())
                .await
        }
        None => {
            let h = r
                .hubble()
                .ok_or_else(|| eyre!("hubble not enabled; pass --host"))?;
            h.fetch_repo(did.to_owned(), cfg.fetch.clone()).await
        }
    }
    .map_err(|e| eyre!("fetch {did}: {e}"))?;
    let fetched_ms = started.elapsed().as_millis();
    let (bytes, spilled) = (body.len(), body.spilled());

    let parse_started = std::time::Instant::now();
    let receipt = sink
        .ingest(did, body)
        .await
        .map_err(|e| eyre!("parse {did}: {e}"))?;
    let parse_ms = parse_started.elapsed().as_millis();

    println!("did       {did}");
    println!("source    {}", host.unwrap_or("hubble"));
    println!(
        "bytes     {bytes} ({})",
        if spilled {
            "spilled to disk"
        } else {
            "in memory"
        }
    );
    println!("fetch     {fetched_ms} ms");
    println!("parse     {parse_ms} ms");
    println!("rev       {}", receipt.rev);
    println!(
        "records   {} (+{} filtered by allowlist)",
        receipt.records, receipt.filtered
    );
    if let Some(h) = r.hubble() {
        if let Ok(info) = h.repo_info(did).await {
            println!(
                "hubble    records={:?} rev={:?} state={:?} pds={:?}",
                info.archive.as_ref().and_then(|a| a.records),
                info.sync_state.as_ref().and_then(|s| s.rev.clone()),
                info.sync_state.as_ref().and_then(|s| s.state.clone()),
                info.pds,
            );
        }
    }
    Ok(())
}
