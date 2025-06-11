use std::fmt;
use std::ops::{Add, Sub};
use std::sync::LazyLock;

use bytes::Bytes;
use fjall::compaction::{Fifo, Strategy};
use fjall::{Keyspace, PartitionCreateOptions, Slice};
use thingbuf::{Recycle, mpsc};

use crate::config::{
    BLOCK_SIZE, CACHE_SIZE, DISK_SIZE, FSYNC_MS, MEMTABLE_SIZE, TTL_SECONDS, WRITE_BUFFER_SIZE,
};

pub type MessageSender = mpsc::blocking::Sender<Message, MessageRecycle>;
pub type MessageReceiver = mpsc::blocking::Receiver<Message, MessageRecycle>;

#[expect(clippy::unwrap_used)]
pub static DB: LazyLock<Keyspace> = LazyLock::new(|| {
    let db = fjall::Config::new("db")
        .cache_size(CACHE_SIZE)
        .max_write_buffer_size(WRITE_BUFFER_SIZE)
        .fsync_ms(FSYNC_MS)
        .open()
        .unwrap();
    db.open_partition("firehose", firehose_options()).unwrap();
    db.open_partition("queue", PartitionCreateOptions::default()).unwrap();
    db.open_partition("repos", PartitionCreateOptions::default()).unwrap();
    db
});

fn firehose_options() -> PartitionCreateOptions {
    PartitionCreateOptions::default()
        .manual_journal_persist(true)
        .compaction_strategy(Strategy::Fifo(Fifo::new(DISK_SIZE, TTL_SECONDS)))
        .max_memtable_size(MEMTABLE_SIZE)
        .block_size(BLOCK_SIZE)
}

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

    fn recycle(&self, _: &mut Message) {}
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

impl From<Slice> for Cursor {
    #[inline]
    fn from(value: Slice) -> Self {
        Self(value.as_ref().try_into().unwrap_or_default())
    }
}

impl From<Cursor> for Slice {
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
