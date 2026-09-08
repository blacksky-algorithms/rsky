//! firehose sequence number aka cursor
//!
//! "fc/"||<hostname> => <seq:u64>||<persisted_at:u64>
//!
//! `seq` is the highest sequence number where every lower one is known to have
//! been acked.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::firehose::Seq;
use crate::{Host, StorageBatch, StorageEngine, StorageError};

use super::{DecodeError, LoadError, PREFIX_FIREHOSE_CURSOR};

impl Seq {
    pub fn from_bytes(bytes: &[u8; 8]) -> Self {
        Self(u64::from_be_bytes(*bytes))
    }
    pub fn to_bytes(self) -> [u8; 8] {
        self.0.to_be_bytes()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorState {
    pub seq: Seq,
    pub persisted_at: SystemTime,
}

impl CursorState {
    fn key(host: &Host) -> Vec<u8> {
        let host = host.name().as_str().as_bytes();
        let mut k = Vec::with_capacity(PREFIX_FIREHOSE_CURSOR.len() + host.len());
        k.extend_from_slice(PREFIX_FIREHOSE_CURSOR);
        k.extend_from_slice(host);
        k
    }

    fn encode(&self) -> Vec<u8> {
        let ms = self
            .persisted_at
            .duration_since(UNIX_EPOCH)
            .expect("to be persisting cursors after the unix epoch")
            .as_millis() as u64;
        let mut out = Vec::with_capacity(8 + 8);
        out.extend_from_slice(&self.seq.to_bytes());
        out.extend_from_slice(&ms.to_be_bytes());
        out
    }

    fn decode(v: &[u8]) -> Result<Self, DecodeError> {
        let (seq_bytes, rest) = v
            .split_first_chunk::<8>()
            .ok_or(DecodeError::InputTooShort)?;
        let (persisted_at_bytes, extra) = rest
            .split_first_chunk::<8>()
            .ok_or(DecodeError::InputTooShort)?;
        if !extra.is_empty() {
            return Err(DecodeError::BadBytes(v.to_vec()));
        }
        let persiste_at_ms = u64::from_be_bytes(*persisted_at_bytes);
        Ok(Self {
            seq: Seq::from_bytes(seq_bytes),
            persisted_at: UNIX_EPOCH + Duration::from_millis(persiste_at_ms),
        })
    }

    pub fn load<S: StorageEngine>(
        storage: &S,
        host: &Host,
    ) -> Result<Option<Self>, LoadError<S::Error>> {
        let Some(bytes) = storage.get(&Self::key(host)).map_err(LoadError::Storage)? else {
            return Ok(None);
        };
        Ok(Some(Self::decode(&bytes)?))
    }

    pub fn store<E: StorageError, B: StorageBatch<E>>(&self, host: &Host, batch: &mut B) {
        batch.put(&Self::key(host), &self.encode());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::host::Hostname;
    use crate::storage::engine::mem::MemEngine;

    fn h(s: &str) -> Arc<Host> {
        Arc::new(Host::raw(Hostname::new(s)))
    }
    fn t(ms: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(ms)
    }
    fn cs(seq: u64, persisted_at_ms: u64) -> CursorState {
        CursorState {
            seq: Seq(seq),
            persisted_at: t(persisted_at_ms),
        }
    }

    #[test]
    fn unstored_loads_as_none() {
        let eng = MemEngine::new();
        let host = h("relay.example");
        assert_eq!(CursorState::load(&eng, &host).unwrap(), None);
    }

    #[test]
    fn roundtrips() {
        let eng = MemEngine::new();
        let host = h("relay.example");
        let saved = cs(12345, 1_700_000_000_000);
        let mut b = eng.batch();
        saved.store(&host, &mut b);
        b.commit().unwrap();

        let got = CursorState::load(&eng, &host).unwrap().expect("present");
        assert_eq!(got, saved);
    }

    #[test]
    fn cursor_is_host_scoped() {
        let eng = MemEngine::new();
        let r1 = h("relay-a.example");
        let r2 = h("relay-b.example");
        let mut b = eng.batch();
        cs(100, 1).store(&r1, &mut b);
        cs(200, 2).store(&r2, &mut b);
        b.commit().unwrap();
        assert_eq!(CursorState::load(&eng, &r1).unwrap().unwrap().seq, Seq(100));
        assert_eq!(CursorState::load(&eng, &r2).unwrap().unwrap().seq, Seq(200));
    }

    #[test]
    fn overwrite_replaces() {
        let eng = MemEngine::new();
        let host = h("relay.example");
        let mut b = eng.batch();
        cs(1, 1).store(&host, &mut b);
        b.commit().unwrap();
        let mut b = eng.batch();
        let later = cs(999, 2);
        later.store(&host, &mut b);
        b.commit().unwrap();
        assert_eq!(CursorState::load(&eng, &host).unwrap().unwrap(), later);
    }

    #[test]
    fn truncated_value_errors() {
        let eng = MemEngine::new();
        let host = h("relay.example");
        let mut b = eng.batch();
        // shorter than the 16-byte encoded length
        b.put(&CursorState::key(&host), &[0u8; 10]);
        b.commit().unwrap();
        let err = CursorState::load(&eng, &host).unwrap_err();
        assert!(matches!(err, LoadError::Decode(DecodeError::InputTooShort)));
    }

    #[test]
    fn trailing_garbage_errors() {
        let eng = MemEngine::new();
        let host = h("relay.example");
        let mut b = eng.batch();
        // valid 16-byte encoding + one extra byte. extras shouldn't be
        // silently ignored — that would obscure a real bug.
        let mut v = cs(42, 100).encode();
        v.push(0xFF);
        b.put(&CursorState::key(&host), &v);
        b.commit().unwrap();
        let err = CursorState::load(&eng, &host).unwrap_err();
        assert!(matches!(err, LoadError::Decode(DecodeError::BadBytes(_))));
    }
}
