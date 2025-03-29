use http::Uri;
use rtrb::{Consumer, Producer};
use thingbuf::{Recycle, mpsc};

pub type CrawlRequestSender = Producer<CrawlRequest>;
pub type CrawlRequestReceiver = Consumer<CrawlRequest>;
pub type MessageSender = mpsc::blocking::Sender<Message, MessageRecycle>;
pub type MessageReceiver = mpsc::blocking::Receiver<Message, MessageRecycle>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FeedId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cursor(pub usize);

#[derive(Debug)]
pub struct CrawlRequest {
    pub uri: Uri,
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
