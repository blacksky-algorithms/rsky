use std::net::{SocketAddr, TcpStream};

use rtrb::{Consumer, Producer};
use tungstenite::stream::MaybeTlsStream;

use crate::types::Cursor;

pub type CommandSender = Producer<Command>;
pub type CommandReceiver = Consumer<Command>;
pub type SubscribeReposSender = Producer<SubscribeRepos>;
pub type SubscribeReposReceiver = Consumer<SubscribeRepos>;

#[derive(Debug)]
pub struct SubscribeRepos {
    pub addr: SocketAddr,
    pub stream: MaybeTlsStream<TcpStream>,
    pub cursor: Option<Cursor>,
}

#[expect(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum Command {
    Connect(SubscribeRepos),
    Shutdown,
}
