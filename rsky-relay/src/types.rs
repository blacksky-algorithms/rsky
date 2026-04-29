use std::fmt;
use std::ops::{Add, Sub};
use std::sync::LazyLock;

use bytes::Bytes;
use fjall::compaction::{Fifo, Strategy};
use fjall::{Keyspace, PartitionCreateOptions, Slice};
use thingbuf::{Recycle, mpsc};

use crate::config::{
    BLOCK_SIZE, CACHE_SIZE, DISK_SIZE, FSYNC_MS, MEMTABLE_SIZE, QUEUE_DISK_SIZE, QUEUE_TTL_SECONDS,
    TTL_SECONDS, WRITE_BUFFER_SIZE,
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
    db.open_partition("queue", queue_options()).unwrap();
    #[cfg(not(feature = "labeler"))]
    db.open_partition("repos", PartitionCreateOptions::default()).unwrap();
    db
});

fn queue_options() -> PartitionCreateOptions {
    PartitionCreateOptions::default()
        .compaction_strategy(Strategy::Fifo(Fifo::new(QUEUE_DISK_SIZE, QUEUE_TTL_SECONDS)))
}

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

    /// Non-mutating `+1`; saturates at `u64::MAX` to keep fjall key ordering monotonic.
    #[inline]
    pub const fn successor(self) -> Self {
        let value = u64::from_be_bytes(self.0).saturating_add(1);
        Self(value.to_be_bytes())
    }

    /// In-place `+1`; saturates at `u64::MAX`. Prefer `successor` for non-mutating reads.
    #[inline]
    pub const fn next(&mut self) -> Self {
        let value = u64::from_be_bytes(self.0).saturating_add(1);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_get_round_trips_u64() {
        for v in [0u64, 1, 42, u64::MAX / 2, u64::MAX - 1, u64::MAX] {
            assert_eq!(Cursor::from(v).get(), v);
        }
    }

    #[test]
    fn successor_does_not_mutate_self() {
        let c = Cursor::from(5);
        let s = c.successor();
        assert_eq!(s.get(), 6);
        assert_eq!(c.get(), 5);
    }

    #[test]
    fn successor_saturates_at_u64_max() {
        assert_eq!(Cursor::from(u64::MAX).successor().get(), u64::MAX);
    }

    #[test]
    fn next_mutates_and_returns_new_value() {
        let mut c = Cursor::from(10);
        let returned = c.next();
        assert_eq!(returned.get(), 11);
        assert_eq!(c.get(), 11);
    }

    #[test]
    fn next_saturates_at_u64_max() {
        let mut c = Cursor::from(u64::MAX);
        assert_eq!(c.next().get(), u64::MAX);
        assert_eq!(c.get(), u64::MAX);
    }

    #[test]
    fn add_sub_round_trip() {
        let c = Cursor::from(100);
        assert_eq!((c + 50).get(), 150);
        assert_eq!((c - 25).get(), 75);
    }

    #[test]
    fn equality_and_default() {
        assert_eq!(Cursor::default().get(), 0);
        assert_eq!(Cursor::from(7), Cursor::from(7));
        assert_ne!(Cursor::from(7), Cursor::from(8));
    }

    #[test]
    fn debug_and_display_match_u64() {
        let c = Cursor::from(123);
        assert_eq!(format!("{c:?}"), "123");
        assert_eq!(format!("{c}"), "123");
    }

    #[test]
    fn slice_round_trip() {
        let c = Cursor::from(0xdead_beef);
        let s: Slice = c.into();
        let c2: Cursor = s.into();
        assert_eq!(c, c2);
    }

    #[test]
    fn slice_too_short_yields_default() {
        let s: Slice = (&[1u8, 2, 3][..]).into();
        assert_eq!(Cursor::from(s), Cursor::default());
    }

    #[test]
    fn as_ref_returns_be_bytes() {
        let c = Cursor::from(1);
        assert_eq!(c.as_ref(), &1u64.to_be_bytes());
    }

    #[test]
    fn into_u64_round_trips() {
        let c = Cursor::from(42);
        let v: u64 = c.into();
        assert_eq!(v, 42);
    }

    #[test]
    fn copy_clone_semantics() {
        let c = Cursor::from(9);
        let d = c;
        let e = c;
        assert_eq!(d, e);
        assert_eq!(c.get(), 9);
    }

    #[test]
    fn message_recycle_no_op() {
        let recycler = MessageRecycle;
        let mut msg = recycler.new_element();
        msg.data = Bytes::from_static(b"x");
        msg.hostname = "h".to_owned();
        // recycle is a no-op; message must be untouched.
        recycler.recycle(&mut msg);
        assert_eq!(msg.data, Bytes::from_static(b"x"));
        assert_eq!(msg.hostname, "h");
    }
}
