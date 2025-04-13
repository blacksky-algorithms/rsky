use std::fmt;
use std::ops::{Add, Sub};
use std::sync::LazyLock;

use sled::{Db, IVec, Mode};
use thingbuf::{Recycle, mpsc};
use tungstenite::Bytes;
use zerocopy::big_endian::U64;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

pub static DB: LazyLock<Db> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used)]
    sled::Config::new().path("db").use_compression(true).mode(Mode::HighThroughput).open().unwrap()
});

pub type MessageSender = mpsc::blocking::Sender<Message, MessageRecycle>;
pub type MessageReceiver = mpsc::blocking::Receiver<Message, MessageRecycle>;

#[derive(Debug)]
pub struct Message {
    pub data: Bytes,
    pub hostname: String,
}

#[derive(Debug)]
pub struct MessageRecycle;

impl Recycle<Message> for MessageRecycle {
    fn new_element(&self) -> Message {
        Message { data: Bytes::new(), hostname: String::new() }
    }

    fn recycle(&self, element: &mut Message) {
        element.data.clear();
    }
}

#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C, packed)]
pub struct TimedMessage {
    pub timestamp: U64,
    pub data: [u8],
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Cursor([u8; 8]);

impl Cursor {
    #[inline]
    pub const fn get(self) -> u64 {
        u64::from_be_bytes(self.0)
    }

    #[inline]
    pub fn next(&mut self) -> Self {
        let value = u64::from_be_bytes(self.0) + 1;
        self.0 = value.to_be_bytes();
        *self
    }
}

impl From<IVec> for Cursor {
    #[inline]
    fn from(value: IVec) -> Self {
        Self(value.as_ref().try_into().unwrap_or_default())
    }
}

impl From<Cursor> for IVec {
    #[inline]
    fn from(value: Cursor) -> Self {
        (&value.0).into()
    }
}

impl AsRef<[u8]> for Cursor {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<u64> for Cursor {
    #[inline]
    fn from(value: u64) -> Self {
        Self(value.to_be_bytes())
    }
}

impl From<Cursor> for u64 {
    #[inline]
    fn from(value: Cursor) -> Self {
        Self::from_be_bytes(value.0)
    }
}

impl Add<u64> for Cursor {
    type Output = Self;

    #[inline]
    fn add(self, rhs: u64) -> Self::Output {
        let value = u64::from_be_bytes(self.0) + rhs;
        Self(value.to_be_bytes())
    }
}

impl Sub<u64> for Cursor {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: u64) -> Self::Output {
        let value = u64::from_be_bytes(self.0) - rhs;
        Self(value.to_be_bytes())
    }
}

impl fmt::Debug for Cursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(f)
    }
}

impl fmt::Display for Cursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(f)
    }
}
