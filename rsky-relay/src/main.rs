use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use color_eyre::Result;

use mimalloc::MiMalloc;
use rustls::crypto::aws_lc_rs::default_provider;
use signal_hook::consts::{SIGINT, TERM_SIGNALS};
use signal_hook::flag;
use signal_hook::iterator::SignalsInfo;
use signal_hook::iterator::exfiltrator::WithOrigin;

use rsky_relay::{
    CrawlerManager, MessageRecycle, PublisherManager, SHUTDOWN, Server, ValidatorManager,
};

const CAPACITY1: usize = 1 << 16;
const CAPACITY2: usize = 1 << 10;
const WORKERS: usize = 4;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[tokio::main]
pub async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    default_provider().install_default().unwrap();

    let terminate_now = Arc::new(AtomicBool::new(false));
    flag::register_conditional_shutdown(SIGINT, 1, Arc::clone(&terminate_now))?;
    flag::register(SIGINT, Arc::clone(&terminate_now))?;

    let (message_tx, message_rx) =
        thingbuf::mpsc::blocking::with_recycle(CAPACITY1, MessageRecycle);
    let (request_crawl_tx, request_crawl_rx) = rtrb::RingBuffer::new(CAPACITY2);
    let (subscribe_repos_tx, subscribe_repos_rx) = rtrb::RingBuffer::new(CAPACITY2);
    let mut validator = ValidatorManager::new(message_rx).await?;
    let crawler = CrawlerManager::new(WORKERS, message_tx, request_crawl_rx)?;
    let publisher = PublisherManager::new(WORKERS, &mut validator, subscribe_repos_rx)?;
    let server = Server::new(request_crawl_tx, subscribe_repos_tx)?;
    thread::scope(move |s| {
        tokio::spawn(validator.run());
        thread::Builder::new().name("rsky-crawl".into()).spawn_scoped(s, move || crawler.run())?;
        thread::Builder::new().name("rsky-pub".into()).spawn_scoped(s, move || publisher.run())?;
        thread::Builder::new().name("rsky-server".into()).spawn_scoped(s, move || server.run())?;
        let mut signals =
            SignalsInfo::<WithOrigin>::new(TERM_SIGNALS).expect("failed to init signals");
        for signal_info in &mut signals {
            if TERM_SIGNALS.contains(&signal_info.signal) {
                break;
            }
        }
        tracing::info!("shutting down");
        SHUTDOWN.store(true, Ordering::Relaxed);
        Ok(())
    })
}
