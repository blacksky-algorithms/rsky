use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use color_eyre::Result;

use rsky_relay::{CrawlerManager, MessageRecycle, PublisherManager, SHUTDOWN, Server};
use signal_hook::consts::{SIGINT, TERM_SIGNALS};
use signal_hook::flag;
use signal_hook::iterator::SignalsInfo;
use signal_hook::iterator::exfiltrator::WithOrigin;

const CAPACITY1: usize = 1 << 20;
const CAPACITY2: usize = 1 << 10;
const WORKERS: usize = 4;

pub fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let terminate_now = Arc::new(AtomicBool::new(false));
    flag::register_conditional_shutdown(SIGINT, 1, Arc::clone(&terminate_now))?;
    flag::register(SIGINT, Arc::clone(&terminate_now))?;

    let (message_tx, message_rx) =
        thingbuf::mpsc::blocking::with_recycle(CAPACITY1, MessageRecycle);
    let (request_crawl_tx, request_crawl_rx) = rtrb::RingBuffer::new(CAPACITY2);
    let (subscribe_repos_tx, subscribe_repos_rx) = rtrb::RingBuffer::new(CAPACITY2);
    let crawler = CrawlerManager::new(WORKERS, message_tx, request_crawl_rx)?;
    let publisher = PublisherManager::new(WORKERS, message_rx, subscribe_repos_rx)?;
    let server = Server::new(request_crawl_tx, subscribe_repos_tx)?;
    thread::scope(move |s| {
        s.spawn(move || crawler.run());
        s.spawn(move || publisher.run());
        s.spawn(move || server.run());
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
