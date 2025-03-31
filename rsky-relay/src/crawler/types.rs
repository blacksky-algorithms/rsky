use std::net::TcpStream;

use http::Uri;
use magnetic::buffer::dynamic::DynamicBufferP2;
use magnetic::mpsc::{MPSCConsumer, MPSCProducer};
use rtrb::{Consumer, Producer};
use tungstenite::WebSocket;
use tungstenite::stream::MaybeTlsStream;

use crate::types::Cursor;

pub type Client = WebSocket<MaybeTlsStream<TcpStream>>;
pub type CommandSender = Producer<Command>;
pub type CommandReceiver = Consumer<Command>;
pub type StatusSender = MPSCProducer<Status, DynamicBufferP2<Status>>;
pub type StatusReceiver = MPSCConsumer<Status, DynamicBufferP2<Status>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct WorkerId(pub usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocalId(pub usize);

#[derive(Debug, Clone)]
pub struct Config {
    pub uri: Uri,
    pub cursor: Cursor,
    pub worker_id: WorkerId,
    pub local_id: LocalId,
}

#[derive(Debug, Clone)]
pub enum Command {
    Connect(Config),
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum Status {}
