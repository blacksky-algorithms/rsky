use std::collections::VecDeque;
use std::io;
use std::net::TcpStream;
use std::os::fd::{AsFd, BorrowedFd};
use std::time::{Duration, Instant};

use thingbuf::mpsc;
use thiserror::Error;
use tungstenite::handshake::server::NoCallback;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{HandshakeError, Message, ServerHandshake};

use crate::publisher::types::{Client, Config, StatusSender};

const MAX_LEN: usize = 1 << 10;
const MAX_DUR: Duration = Duration::from_secs(300);

#[derive(Debug, Error)]
pub enum ConnectionError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("handshake error: {0}")]
    Handshake(#[from] HandshakeError<ServerHandshake<MaybeTlsStream<TcpStream>, NoCallback>>),
    #[error("tungstenite error: {0}")]
    Tungstenite(#[from] tungstenite::Error),
    #[error("thingbuf error: {0}")]
    Thingbuf(#[from] mpsc::errors::TrySendError),
}

pub struct Connection {
    client: Client,
    queue: VecDeque<(Instant, Message)>,
    status_tx: StatusSender,
}

impl AsFd for Connection {
    #[inline]
    fn as_fd(&self) -> BorrowedFd<'_> {
        match self.client.get_ref() {
            MaybeTlsStream::Plain(stream) => stream.as_fd(),
            MaybeTlsStream::Rustls(stream) => stream.get_ref().as_fd(),
            _ => todo!(),
        }
    }
}

impl Connection {
    pub fn connect(config: Config, status_tx: StatusSender) -> Result<Self, ConnectionError> {
        let mut client = tungstenite::accept(config.stream)?;
        // TODO: publisher state management
        client.send(Message::text(format!("{:?}", config.cursor)))?;
        match client.get_ref() {
            MaybeTlsStream::Rustls(stream) => {
                stream.get_ref().set_nonblocking(true)?;
            }
            MaybeTlsStream::Plain(stream) => {
                stream.set_nonblocking(true)?;
            }
            _ => {}
        }
        Ok(Self { client, queue: VecDeque::new(), status_tx })
    }

    pub fn close(&mut self) -> Result<(), ConnectionError> {
        self.client.close(None)?;
        self.client.flush()?;
        Ok(())
    }

    pub fn send(&mut self, input: &[u8]) -> Result<(), ConnectionError> {
        match self.client.send(Message::binary(input.to_vec())) {
            Ok(()) => (),
            Err(tungstenite::Error::WriteBufferFull(msg)) => {
                self.queue.push_back((Instant::now(), msg));
                if self.queue.len() > MAX_LEN {
                    return self.close();
                }
            }
            Err(tungstenite::Error::Io(e)) if e.kind() == io::ErrorKind::WouldBlock => {
                if let Some((instant, _)) = self.queue.front() {
                    if instant.elapsed() > MAX_DUR {
                        return self.close();
                    }
                }
            }
            Err(err) => Err(err)?,
        }
        Ok(())
    }

    pub fn poll(&mut self) -> Result<(), ConnectionError> {
        while let Some((instant, msg)) = self.queue.pop_front() {
            match self.client.send(msg) {
                Ok(()) => (),
                Err(tungstenite::Error::WriteBufferFull(msg)) => {
                    self.queue.push_back((instant, msg));
                    break;
                }
                Err(tungstenite::Error::Io(e)) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(err) => Err(err)?,
            }
        }
        Ok(())
    }
}
