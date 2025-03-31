mod crawler;
mod publisher;
mod server;
mod types;

use std::sync::atomic::AtomicBool;

pub static SHUTDOWN: AtomicBool = AtomicBool::new(false);

pub use crawler::Manager as CrawlerManager;
pub use publisher::Manager as PublisherManager;
pub use server::Server;
pub use types::{MessageRecycle, RequestCrawl};
