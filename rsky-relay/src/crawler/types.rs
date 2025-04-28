use magnetic::buffer::dynamic::DynamicBufferP2;
use magnetic::mpsc::{MPSCConsumer, MPSCProducer};
use rtrb::{Consumer, Producer};
use serde::Deserialize;

use crate::types::Cursor;

pub type CommandSender = Producer<Command>;
pub type CommandReceiver = Consumer<Command>;
pub type StatusSender = MPSCProducer<Status, DynamicBufferP2<Status>>;
pub type StatusReceiver = MPSCConsumer<Status, DynamicBufferP2<Status>>;
pub type RequestCrawlSender = Producer<RequestCrawl>;
pub type RequestCrawlReceiver = Consumer<RequestCrawl>;

#[derive(Debug, Deserialize)]
pub struct RequestCrawl {
    pub hostname: String,
    #[serde(skip)]
    pub(super) cursor: Option<Cursor>,
}

#[derive(Debug)]
pub enum Command {
    Connect(RequestCrawl),
    Shutdown,
}

#[derive(Debug)]
pub enum Status {
    Disconnected(usize, String),
}
