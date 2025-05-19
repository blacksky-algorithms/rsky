use std::net::TcpStream;

use magnetic::buffer::dynamic::DynamicBufferP2;
use magnetic::mpsc::{MPSCConsumer, MPSCProducer};
use rtrb::{Consumer, Producer};
use serde::Deserialize;
use tungstenite::handshake::MidHandshake;
use tungstenite::handshake::client::Response;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{ClientHandshake, HandshakeError, WebSocket};

use crate::types::Cursor;

pub type MaybeTlsTcpStream = MaybeTlsStream<TcpStream>;
pub type WebSocketClient = WebSocket<MaybeTlsTcpStream>;
pub type Handshaking = MidHandshake<ClientHandshake<MaybeTlsTcpStream>>;
pub type MaybeHandshake = Result<WebSocketClient, Handshaking>;
pub type HandshakeResult = Result<MaybeHandshake, tungstenite::Error>;

pub trait DecomposeError {
    fn decompose(self) -> HandshakeResult;
}

impl DecomposeError
    for Result<(WebSocketClient, Response), HandshakeError<ClientHandshake<MaybeTlsTcpStream>>>
{
    fn decompose(self) -> HandshakeResult {
        match self {
            Ok((client, _)) => Ok(Ok(client)),
            Err(HandshakeError::Interrupted(handshaking)) => Ok(Err(handshaking)),
            Err(HandshakeError::Failure(err)) => Err(err),
        }
    }
}

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
    pub cursor: Option<Cursor>,
}

#[derive(Debug)]
pub enum Command {
    Connect(RequestCrawl),
}

#[derive(Debug)]
pub enum Status {
    Disconnected { worker_id: usize, hostname: String, connected: bool },
}
