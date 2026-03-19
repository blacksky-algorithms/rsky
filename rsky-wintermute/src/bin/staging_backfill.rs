//! Standalone backfiller for offline staging mode.
//!
//! Reads repos from the Fjall queue and writes records into UNLOGGED staging tables
//! on a temporary PostgreSQL server. No indexes, no constraints, no ON CONFLICT.
//! The staging data is later sorted and merged into production by `staging_merge`.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use clap::Parser;
use color_eyre::Result;
use signal_hook::consts::TERM_SIGNALS;
use signal_hook::iterator::SignalsInfo;
use signal_hook::iterator::exfiltrator::WithOrigin;

use rsky_wintermute::backfiller::BackfillerManager;
use rsky_wintermute::storage::Storage;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(Debug, Parser)]
#[command(name = "staging_backfill")]
#[command(about = "Backfill repos into UNLOGGED staging tables for offline build + sorted merge")]
struct Args {
    /// Staging database URL (UNLOGGED tables, no indexes)
    #[arg(long, env = "STAGING_DATABASE_URL")]
    staging_database_url: String,

    /// Path to Fjall database directory containing the repo_backfill queue
    #[arg(long, default_value = "/data/backfill/backfill_cache")]
    db_path: PathBuf,
}

fn main() -> Result<()> {
    color_eyre::install()?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args = Args::parse();

    // Force staging mode and direct write before any config is read
    unsafe {
        std::env::set_var("BACKFILLER_STAGING_MODE", "true");
        std::env::set_var("BACKFILLER_DIRECT_WRITE", "true");
    }

    // Install TLS provider before any network calls
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("failed to install rustls crypto provider");

    // Handle shutdown signals in a background thread
    std::thread::spawn(|| {
        let mut signals =
            SignalsInfo::<WithOrigin>::new(TERM_SIGNALS).expect("failed to register signals");
        if signals.into_iter().next().is_some() {
            tracing::info!("shutdown signal received");
            rsky_wintermute::SHUTDOWN.store(true, Ordering::Relaxed);
        }
    });

    tracing::info!(
        "staging backfill starting, db_path={}, staging_url={}",
        args.db_path.display(),
        args.staging_database_url.split('@').last().unwrap_or("***")
    );

    let storage = Arc::new(Storage::new(Some(args.db_path))?);

    let queue_len = storage.repo_backfill_len()?;
    tracing::info!("repo_backfill queue length: {queue_len}");

    let backfiller = BackfillerManager::new(storage, &args.staging_database_url)?;
    backfiller.run()?;

    tracing::info!("staging backfill complete");
    Ok(())
}
