use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, ScopedJoinHandle};
use std::time::Duration;

use clap::Parser;
use color_eyre::Result;
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

use rsky_relay::config::{CAPACITY_MSGS, CAPACITY_REQS, WORKERS_CRAWLERS, WORKERS_PUBLISHERS};
use rsky_relay::{
    CrawlerManager, MessageRecycle, PublisherManager, RelayError, SHUTDOWN, Server,
    ValidatorManager,
};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

const SLEEP: Duration = Duration::from_millis(10);

#[derive(Debug, clap::Parser)]
pub struct Args {
    #[clap(short, long, requires = "private_key")]
    certs: Option<PathBuf>,
    #[clap(short, long, requires = "certs")]
    private_key: Option<PathBuf>,
    #[cfg(not(feature = "labeler"))]
    #[clap(long)]
    no_plc_export: bool,
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
    let handle = tokio::spawn(validator.run());
    let crawler = CrawlerManager::new(WORKERS_CRAWLERS, &message_tx, request_crawl_rx)?;
    let publisher = PublisherManager::new(WORKERS_PUBLISHERS, subscribe_repos_rx)?;
    #[expect(clippy::vec_init_then_push)]
    let ret = thread::scope(move |s| {
        let mut handles = Vec::<ScopedJoinHandle<'_, Result<_, RelayError>>>::new();
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
    });
    handle.await??;
    ret
}
