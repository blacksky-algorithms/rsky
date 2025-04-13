use rtrb::{Consumer, Producer};
use serde::Deserialize;

use crate::types::Cursor;

pub type CommandSender = Producer<Command>;
pub type CommandReceiver = Consumer<Command>;
pub type RequestCrawlSender = Producer<RequestCrawl>;
pub type RequestCrawlReceiver = Consumer<RequestCrawl>;

#[derive(Debug, Deserialize)]
pub struct RequestCrawl {
    pub hostname: String,
    #[serde(skip)]
    pub cursor: Option<Cursor>,
}

#[derive(Debug)]
pub enum Command {
    Connect(RequestCrawl),
    Shutdown,
}
