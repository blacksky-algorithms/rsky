use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, ScopedJoinHandle};
use std::time::Duration;

use clap::Parser;
use color_eyre::Result;
use color_eyre::eyre::eyre;
use file_rotate::compression::Compression;
use file_rotate::suffix::{AppendTimestamp, FileLimit};
use file_rotate::{ContentLimit, FileRotate, TimeFrequency};
use mimalloc::MiMalloc;
use rustls::crypto::aws_lc_rs::default_provider;
use signal_hook::consts::{SIGINT, TERM_SIGNALS};
use signal_hook::flag;
use signal_hook::iterator::SignalsInfo;
use signal_hook::iterator::exfiltrator::WithOrigin;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use rsky_relay::config::{
    CAPACITY_MSGS, CAPACITY_REQS, METRICS_LISTEN, WORKERS_CRAWLERS, WORKERS_PUBLISHERS,
};
use rsky_relay::{
    CrawlerManager, Health, MessageRecycle, PublisherManager, RelayError, SHUTDOWN, Server,
    ValidatorManager, health, metrics, migrate, set_health,
};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

const SLEEP: Duration = Duration::from_millis(10);
const RELAY_DB: &str = "relay.db";

#[derive(Debug, clap::Parser)]
pub struct Args {
    #[clap(short, long, requires = "private_key")]
    certs: Option<PathBuf>,
    #[clap(short, long, requires = "certs")]
    private_key: Option<PathBuf>,
    #[cfg(not(feature = "labeler"))]
    #[clap(long)]
    no_plc_export: bool,
    /// Copy of the 0.2.x `db/` taken while the relay was stopped; required on the
    /// first start after an upgrade and after every `--migrate-down`.
    #[clap(long)]
    legacy_source: Option<PathBuf>,
    /// Rewrite the deferred queue and SQLite cursors into the 0.2.x layout and exit.
    #[clap(long)]
    migrate_down: bool,
}

#[tokio::main]
pub async fn main() -> Result<()> {
    let file_appender = FileRotate::new(
        "rsky-relay.log",
        AppendTimestamp::default(FileLimit::MaxFiles(7)),
        ContentLimit::Time(TimeFrequency::Daily),
        Compression::OnRotate(0),
        None,
    );
    let (json_writer, _guard_json) = tracing_appender::non_blocking(file_appender);
    let (pretty_writer, _guard_pretty) = tracing_appender::non_blocking(std::io::stdout());
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(Layer::new().json().with_ansi(false).with_writer(json_writer))
        .with(Layer::new().pretty().with_writer(pretty_writer))
        .init();
    color_eyre::install()?;

    #[expect(clippy::unwrap_used)]
    default_provider().install_default().unwrap();

    let args = Args::parse();

    if args.migrate_down {
        let stats = migrate::migrate_down(&rsky_relay::db(), Path::new(RELAY_DB))?;
        tracing::info!(?stats, "migrate-down done");
        return Ok(());
    }

    let handle = match metrics::install_recorder() {
        Ok(handle) => Some(handle),
        Err(err) => {
            tracing::error!(%err, "failed to install metrics recorder");
            None
        }
    };
    if let (Some(addr_str), Some(handle)) = (METRICS_LISTEN.as_deref(), handle) {
        match addr_str.parse() {
            Ok(addr) => {
                tokio::spawn(async move {
                    if let Err(err) = health::serve(addr, handle).await {
                        tracing::error!(%err, "health listener exited");
                    }
                });
            }
            Err(err) => tracing::error!(%err, %addr_str, "invalid RELAY_METRICS_LISTEN"),
        }
    }

    reconcile_if_needed(args.legacy_source.as_deref())?;

    let terminate_now = Arc::new(AtomicBool::new(false));
    flag::register_conditional_shutdown(SIGINT, 1, Arc::clone(&terminate_now))?;
    flag::register(SIGINT, Arc::clone(&terminate_now))?;

    let (message_tx, message_rx) =
        thingbuf::mpsc::blocking::with_recycle(CAPACITY_MSGS, MessageRecycle);
    let (request_crawl_tx, request_crawl_rx) = rtrb::RingBuffer::new(CAPACITY_REQS);
    let (subscribe_repos_tx, subscribe_repos_rx) = rtrb::RingBuffer::new(CAPACITY_REQS);
    let validator = ValidatorManager::new(message_rx)?;
    let server =
        Server::new(args.certs.zip(args.private_key), request_crawl_tx, subscribe_repos_tx)?;
    let crawler = CrawlerManager::new(WORKERS_CRAWLERS, &message_tx, request_crawl_rx)?;
    let publisher = PublisherManager::new(WORKERS_PUBLISHERS, subscribe_repos_rx)?;
    let ret = thread::scope(move |s| {
        let mut handles = Vec::<ScopedJoinHandle<'_, Result<_, RelayError>>>::new();
        // The validator owns a thread: its loop never yields for long stretches,
        // which starved every other task when it shared the runtime.
        handles.push(thread::Builder::new().name("rsky-validator".into()).spawn_scoped(
            s,
            move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|err| RelayError::Validator(err.into()))?;
                rt.block_on(validator.run()).map_err(Into::into)
            },
        )?);
        handles.push(
            thread::Builder::new()
                .name("rsky-crawl".into())
                .spawn_scoped(s, move || crawler.run().map_err(Into::into))?,
        );
        handles.push(
            thread::Builder::new()
                .name("rsky-pub".into())
                .spawn_scoped(s, move || publisher.run().map_err(Into::into))?,
        );
        handles.push(
            thread::Builder::new()
                .name("rsky-server".into())
                .spawn_scoped(s, move || server.run().map_err(Into::into))?,
        );
        #[expect(clippy::expect_used)]
        let mut signals =
            SignalsInfo::<WithOrigin>::new(TERM_SIGNALS).expect("failed to init signals");
        let mut validator_died = false;
        'outer: loop {
            for signal_info in signals.pending() {
                if TERM_SIGNALS.contains(&signal_info.signal) {
                    break 'outer;
                }
            }
            if handles[0].is_finished() && !SHUTDOWN.load(Ordering::Relaxed) {
                // A dead validator means a frozen firehose; exiting lets systemd
                // restart the relay instead of serving stale data indefinitely.
                validator_died = true;
                set_health(Health::Dead);
                tracing::error!("validator stopped unexpectedly, shutting down for restart");
                break 'outer;
            }
            for h in &handles[1..] {
                if h.is_finished() {
                    break 'outer;
                }
            }
            thread::sleep(SLEEP);
        }
        tracing::info!("shutting down");
        SHUTDOWN.store(true, Ordering::Relaxed);
        let mut first_err = None;
        for h in handles {
            match h.join() {
                Ok(Ok(())) => {}
                Ok(Err(err)) => {
                    tracing::error!(%err, "component stopped with error");
                    first_err.get_or_insert(eyre!(err));
                }
                Err(_) => {
                    tracing::error!("component panicked");
                    first_err.get_or_insert_with(|| eyre!("component panicked"));
                }
            }
        }
        match first_err {
            Some(err) => Err(err),
            None if validator_died => Err(eyre!("validator stopped unexpectedly")),
            None => Ok(()),
        }
    });
    if ret.is_ok() {
        tracing::info!("validator stopped cleanly");
    }
    ret
}

/// Runs the legacy reconcile before any component starts, or refuses to start
/// when 0.2.x state exists and no stopped copy of `db/` was supplied.
fn reconcile_if_needed(legacy_source: Option<&Path>) -> Result<()> {
    let db = rsky_relay::db();
    if !migrate::needs_reconcile(&db)? {
        return Ok(());
    }
    if !migrate::has_legacy_state(&db, Path::new(RELAY_DB))? {
        migrate::mark_migrated(&db)?;
        return Ok(());
    }
    let Some(source) = legacy_source else {
        return Err(migrate::MigrateError::SourceRequired.into());
    };
    let source = rsky_relay::open_source_keyspace(source)?;
    let stats = migrate::reconcile(&db, &source, Path::new(RELAY_DB))?;
    tracing::info!(?stats, "reconciled legacy state");
    Ok(())
}
