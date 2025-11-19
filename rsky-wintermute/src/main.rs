use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use clap::Parser;
use color_eyre::Result;
use mimalloc::MiMalloc;
use signal_hook::consts::{SIGINT, TERM_SIGNALS};
use signal_hook::flag;
use signal_hook::iterator::SignalsInfo;
use signal_hook::iterator::exfiltrator::WithOrigin;
use tracing_subscriber::EnvFilter;

use rsky_wintermute::SHUTDOWN;
use rsky_wintermute::backfiller::BackfillerManager;
use rsky_wintermute::indexer::IndexerManager;
use rsky_wintermute::ingester::IngesterManager;
use rsky_wintermute::storage::Storage;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

const SLEEP: Duration = Duration::from_millis(10);

#[derive(Debug, clap::Parser)]
pub struct Args {
    #[clap(long, env = "RELAY_HOSTS", value_delimiter = ',')]
    relay_hosts: Vec<String>,

    #[clap(long, env = "LABELER_HOSTS", value_delimiter = ',', default_value = "")]
    labeler_hosts: Vec<String>,

    #[clap(long, env = "DATABASE_URL")]
    database_url: String,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    color_eyre::install()?;

    let args = Args::parse();

    if args.relay_hosts.is_empty() {
        return Err(color_eyre::eyre::eyre!("no relay hosts specified"));
    }

    tracing::info!("starting wintermute");
    tracing::info!("relay hosts: {:?}", args.relay_hosts);
    tracing::info!("database: {}", args.database_url);

    let terminate_now = Arc::new(std::sync::atomic::AtomicBool::new(false));
    flag::register_conditional_shutdown(SIGINT, 1, Arc::clone(&terminate_now))?;
    flag::register(SIGINT, Arc::clone(&terminate_now))?;

    let storage = Arc::new(Storage::new()?);
    let labeler_hosts: Vec<String> = args
        .labeler_hosts
        .into_iter()
        .filter(|h| !h.is_empty())
        .collect();
    let ingester = IngesterManager::new(args.relay_hosts, labeler_hosts, Arc::clone(&storage))?;
    let backfiller = BackfillerManager::new(Arc::clone(&storage))?;
    let indexer = IndexerManager::new(Arc::clone(&storage), args.database_url)?;

    thread::scope(move |s| {
        let handles = vec![
            thread::Builder::new()
                .name("wintermute-ingester".into())
                .spawn_scoped(s, move || ingester.run())?,
            thread::Builder::new()
                .name("wintermute-backfiller".into())
                .spawn_scoped(s, move || backfiller.run())?,
            thread::Builder::new()
                .name("wintermute-indexer".into())
                .spawn_scoped(s, move || indexer.run())?,
        ];

        let mut signals = SignalsInfo::<WithOrigin>::new(TERM_SIGNALS)
            .map_err(|e| color_eyre::eyre::eyre!("failed to init signals: {e}"))?;

        'outer: loop {
            for signal_info in signals.pending() {
                if TERM_SIGNALS.contains(&signal_info.signal) {
                    break 'outer;
                }
            }
            for handle in &handles {
                if handle.is_finished() {
                    break 'outer;
                }
            }
            thread::sleep(SLEEP);
        }

        tracing::info!("shutting down");
        SHUTDOWN.store(true, Ordering::Relaxed);

        for handle in handles {
            if let Ok(res) = handle.join() {
                res?;
            }
        }

        Ok(())
    })
}
