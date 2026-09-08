//! persisted per-pds-host stuff
//!
//! just the sync1.1 compliance ratchet for now
//!
//! "hc/"||<hostname>
//!   => <sync11: u8>
//!
//! non-strict (0) is not written to the db (absent key indicates it)
//!
//! (not super happy with where this landed yet -- probably should jump it right
//! to cbor value encoding, since we'll prob want to add more host properties)

use crate::host::{Host, Sync11State};
use crate::{StorageBatch, StorageEngine, StorageError};

use super::{DecodeError, LoadError, PREFIX_HOST_INFO};

fn build_key(host: &Host) -> Vec<u8> {
    let hostname = host.name().as_str().as_bytes();
    let mut k = Vec::with_capacity(PREFIX_HOST_INFO.len() + hostname.len());
    k.extend_from_slice(PREFIX_HOST_INFO);
    k.extend_from_slice(hostname);
    k
}

fn encode(state: Sync11State) -> Vec<u8> {
    match state {
        Sync11State::Lax => unreachable!("sync11-lax is never persisted"),
        Sync11State::Strict => vec![0x01],
    }
}

fn decode<E: StorageError>(v: &[u8]) -> Result<Sync11State, LoadError<E>> {
    if v == [0x01] {
        return Ok(Sync11State::Strict);
    }
    // only strict is encodeable for now
    Err(LoadError::Decode(DecodeError::BadBytes(v.to_vec())))
}

pub fn load<S: StorageEngine>(
    storage: &S,
    host: &Host,
) -> Result<Sync11State, LoadError<S::Error>> {
    let Some(bytes) = storage.get(&build_key(host)).map_err(LoadError::Storage)? else {
        return Ok(Sync11State::Lax);
    };
    decode(&bytes)
}

/// ratchet a host to sync1.1 strict
pub fn store_strict<E: StorageError, B: StorageBatch<E>>(batch: &mut B, host: &Host) {
    batch.put(&build_key(host), &encode(Sync11State::Strict))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::host::Hostname;
    use crate::storage::engine::mem::MemEngine;

    fn host(s: &str) -> Arc<Host> {
        Arc::new(Host::raw(Hostname::new(s)))
    }

    #[test]
    fn unstored_loads_as_lax() {
        let eng = MemEngine::new();
        let h = host("example.com");
        assert_eq!(h.sync_compliance(&eng).unwrap(), Sync11State::Lax);
    }

    #[test]
    fn ratchet_persists_strict() {
        let eng = MemEngine::new();
        let h = host("example.com");
        let mut b = eng.batch();
        h.ratchet_strict(&mut b);
        b.commit().unwrap();
        // fresh Host (no in-memory cache from the previous one) reloads from
        // storage and sees Strict.
        let h2 = host("example.com");
        assert_eq!(h2.sync_compliance(&eng).unwrap(), Sync11State::Strict);
    }

    #[test]
    fn ratchet_is_idempotent() {
        let eng = MemEngine::new();
        let h = host("example.com");
        let mut b = eng.batch();
        h.ratchet_strict(&mut b);
        h.ratchet_strict(&mut b); // no-op (cache already Strict)
        b.commit().unwrap();
        assert_eq!(h.sync_compliance(&eng).unwrap(), Sync11State::Strict);
    }

    #[test]
    fn ratchet_from_unset_writes_to_batch() {
        // even with the cell still UNSET (no prior sync_compliance call),
        // ratchet swaps to Strict and stages the put. verify by loading from
        // storage via a fresh Host (which bypasses the in-memory cache).
        let eng = MemEngine::new();
        let h = host("example.com");
        let mut b = eng.batch();
        h.ratchet_strict(&mut b);
        b.commit().unwrap();
        let fresh = host("example.com");
        assert_eq!(fresh.sync_compliance(&eng).unwrap(), Sync11State::Strict);
    }

    #[test]
    fn cached_compliance_skips_db_read() {
        let eng = MemEngine::new();
        let h = host("example.com");
        // first call loads Lax (nothing stored) and caches it
        assert_eq!(h.sync_compliance(&eng).unwrap(), Sync11State::Lax);
        // now persist Strict directly, bypassing this Host
        let mut b = eng.batch();
        store_strict(&mut b, &h);
        b.commit().unwrap();
        // second call returns the cached Lax — the in-memory cell is the
        // source of truth in-process, the db is not re-read.
        assert_eq!(h.sync_compliance(&eng).unwrap(), Sync11State::Lax);
        // a fresh Host (no cache) sees the persisted Strict.
        let fresh = host("example.com");
        assert_eq!(fresh.sync_compliance(&eng).unwrap(), Sync11State::Strict);
    }

    #[test]
    fn malformed_value_errors() {
        let eng = MemEngine::new();
        // write a value that's neither Lax-absent nor Strict-encoded.
        let mut b = eng.batch();
        b.put(&build_key(&host("example.com")), &[0x7F]);
        b.commit().unwrap();
        let h = host("example.com");
        let err = h.sync_compliance(&eng).unwrap_err();
        assert!(matches!(err, LoadError::Decode(DecodeError::BadBytes(_))));
    }
}
