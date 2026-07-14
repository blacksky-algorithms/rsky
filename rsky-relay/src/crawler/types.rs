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
    Disconnect(String),
}

#[derive(Debug)]
pub enum Status {
    Disconnected { worker_id: usize, hostname: String, connected: bool },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_crawl_deserializes_and_skips_cursor() {
        // cursor is #[serde(skip)] -> never deserialized from JSON.
        let json = r#"{"hostname":"pds.example","cursor":42}"#;
        let r: RequestCrawl = serde_json::from_str(json).unwrap();
        assert_eq!(r.hostname, "pds.example");
        assert!(r.cursor.is_none());
    }

    #[test]
    fn command_debug_renders_variants() {
        let connect = Command::Connect(RequestCrawl {
            hostname: "h".to_owned(),
            cursor: Some(Cursor::from(1)),
        });
        let disconnect = Command::Disconnect("h".to_owned());
        assert!(format!("{connect:?}").starts_with("Connect("));
        assert!(format!("{disconnect:?}").starts_with("Disconnect("));
    }

    #[test]
    fn status_debug_renders_variant() {
        let s = Status::Disconnected { worker_id: 7, hostname: "h".to_owned(), connected: true };
        assert!(format!("{s:?}").contains("worker_id: 7"));
    }

    #[test]
    fn decompose_handshake_failure_propagates_error() {
        // Construct a fake `tungstenite::Error` and pump it through Decompose.
        let res: Result<
            (WebSocketClient, Response),
            HandshakeError<ClientHandshake<MaybeTlsTcpStream>>,
        > = Err(HandshakeError::Failure(tungstenite::Error::ConnectionClosed));
        let out = res.decompose();
        assert!(matches!(out, Err(tungstenite::Error::ConnectionClosed)));
    }
}
