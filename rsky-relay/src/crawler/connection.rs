use std::io;
use std::os::fd::{AsFd, BorrowedFd};

use thingbuf::mpsc;
use thiserror::Error;
use tungstenite::Message;
use tungstenite::stream::MaybeTlsStream;

use crate::crawler::types::{Client, Config, StatusSender};
use crate::types::MessageSender;

#[derive(Debug, Error)]
pub enum ConnectionError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("tungstenite error: {0}")]
    Tungstenite(#[from] tungstenite::Error),
    #[error("thingbuf error: {0}")]
    Thingbuf(#[from] mpsc::errors::TrySendError),
}

pub struct Connection {
    client: Client,
    config: Config,
    message_tx: MessageSender,
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
    pub fn connect(
        config: Config, message_tx: MessageSender, status_tx: StatusSender,
    ) -> Result<Self, ConnectionError> {
        let (client, _) = tungstenite::connect(&config.uri)?;
        match client.get_ref() {
            MaybeTlsStream::Rustls(stream) => {
                stream.get_ref().set_nonblocking(true)?;
            }
            MaybeTlsStream::Plain(stream) => {
                stream.set_nonblocking(true)?;
            }
            _ => {}
        }
        Ok(Self { client, config, message_tx, status_tx })
    }

    pub fn close(&mut self) -> Result<(), ConnectionError> {
        self.client.close(None)?;
        self.client.flush()?;
        Ok(())
    }

    pub fn poll(&mut self) -> Result<(), ConnectionError> {
        for _ in 0..1024 {
            let msg = match self.client.read() {
                Ok(msg) => msg,
                Err(tungstenite::Error::Io(e)) if e.kind() == io::ErrorKind::WouldBlock => {
                    break;
                }
                Err(err) => Err(err)?,
            };

            let bytes = match msg {
                Message::Binary(bytes) => bytes,
                Message::Close(_) => todo!(),
                _ => {
                    tracing::debug!("unknown message: {msg}");
                    continue;
                },
            };

            let mut slot = self.message_tx.try_send_ref()?;
            slot.data = bytes.into();
            slot.uri = self.config.uri.clone();
        }
        Ok(())
    }
}
