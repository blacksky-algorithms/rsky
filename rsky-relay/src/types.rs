use std::net::TcpStream;
use std::sync::LazyLock;

use http::Uri;
use rtrb::{Consumer, Producer};
use sled::Db;
use thingbuf::{Recycle, mpsc};
use tungstenite::stream::MaybeTlsStream;

pub static DB: LazyLock<Db> =
    LazyLock::new(|| sled::Config::new().path("db").use_compression(true).open().unwrap());

pub type RequestCrawlSender = Producer<RequestCrawl>;
pub type RequestCrawlReceiver = Consumer<RequestCrawl>;
pub type SubscribeReposSender = Producer<SubscribeRepos>;
pub type SubscribeReposReceiver = Consumer<SubscribeRepos>;
pub type MessageSender = mpsc::blocking::Sender<Message, MessageRecycle>;
pub type MessageReceiver = mpsc::blocking::Receiver<Message, MessageRecycle>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FeedId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cursor(pub u64);

#[derive(Debug)]
pub struct RequestCrawl {
    pub uri: Uri,
}

#[derive(Debug)]
pub struct SubscribeRepos {
    pub stream: MaybeTlsStream<TcpStream>,
    pub cursor: Cursor,
}

#[derive(Debug)]
pub struct Message {
    pub data: Vec<u8>,
    pub uri: Uri,
}

#[derive(Debug)]
pub struct MessageRecycle;

impl Recycle<Message> for MessageRecycle {
    fn new_element(&self) -> Message {
        Message { data: Vec::new(), uri: Uri::from_static("example.com") }
    }

    fn recycle(&self, element: &mut Message) {
        element.data.clear();
    }
}
