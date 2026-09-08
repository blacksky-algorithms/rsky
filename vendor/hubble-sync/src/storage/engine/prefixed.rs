//! wrap any StorageEngine to prefix every key
//!
//! hubble-sync uses this to keep its internal state under a strict prefx. apps
//! have to avoid writing under hubble's prefix, and they can use this to help
//! prevent that too, putting all their keys under their own disjoint prefix.

use super::types::{Pair, StorageBatch, StorageEngine, StorageError};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct PrefixedEngine<E> {
    inner: E,
    prefix: &'static [u8],
}

/// wrap a storage engine, prefixing keys on every access
impl<E: StorageEngine> PrefixedEngine<E> {
    pub fn new(inner: E, prefix: &'static [u8]) -> Self {
        Self { inner, prefix }
    }

    /// access the wrapped engine
    ///
    /// same storage, unprefixed
    pub fn inner(&self) -> &E {
        &self.inner
    }

    fn prefixed(&self, k: &[u8]) -> Vec<u8> {
        [self.prefix, k].concat()
    }
}

impl<E: StorageEngine> StorageEngine for PrefixedEngine<E> {
    type Error = E::Error;
    type Batch = PrefixedBatch<E::Batch>;

    fn batch(&self) -> Self::Batch {
        PrefixedBatch {
            inner: self.inner.batch(),
            prefix: self.prefix,
        }
    }

    fn get(&self, k: &[u8]) -> Result<Option<Vec<u8>>, Self::Error> {
        self.inner.get(&self.prefixed(k))
    }
    fn get_queue(&self, k: &[u8]) -> Result<Option<Vec<u8>>, Self::Error> {
        self.inner.get_queue(&self.prefixed(k))
    }
    fn get_counter(&self, k: &[u8]) -> Result<i64, Self::Error> {
        self.inner.get_counter(&self.prefixed(k))
    }

    fn scan_from(
        &self,
        prefix: &[u8],
        from_suffix: &[u8],
    ) -> Box<dyn Iterator<Item = Result<Pair, Self::Error>> + '_> {
        self.inner.scan_from(&self.prefixed(prefix), from_suffix)
    }

    fn scan_from_queue(
        &self,
        prefix: &[u8],
        from_suffix: &[u8],
    ) -> Box<dyn Iterator<Item = Result<Pair, Self::Error>> + '_> {
        self.inner
            .scan_from_queue(&self.prefixed(prefix), from_suffix)
    }

    fn maintenance(&self) -> Result<Option<Duration>, Self::Error> {
        self.inner.maintenance()
    }

    fn report_metrics(&self) {
        self.inner.report_metrics()
    }
}

pub struct PrefixedBatch<B> {
    inner: B,
    prefix: &'static [u8],
}

impl<B> PrefixedBatch<B> {
    /// same atomic batch, unprefixed
    pub fn inner_mut(&mut self) -> &mut B {
        &mut self.inner
    }

    fn prefixed(&self, k: &[u8]) -> Vec<u8> {
        [self.prefix, k].concat()
    }
}

impl<E: StorageError, B: StorageBatch<E>> StorageBatch<E> for PrefixedBatch<B> {
    fn commit(self) -> Result<(), E> {
        self.inner.commit()
    }
    fn put(&mut self, k: &[u8], v: &[u8]) {
        self.inner.put(&self.prefixed(k), v)
    }
    fn delete(&mut self, k: &[u8]) {
        self.inner.delete(&self.prefixed(k))
    }
    fn put_queue(&mut self, k: &[u8], v: &[u8]) {
        self.inner.put_queue(&self.prefixed(k), v)
    }
    fn delete_queue(&mut self, k: &[u8]) {
        self.inner.delete_queue(&self.prefixed(k))
    }
    fn put_counter(&mut self, k: &[u8], count: i64) {
        self.inner.put_counter(&self.prefixed(k), count)
    }
    fn increment_counter(&mut self, k: &[u8], delta: i64) {
        self.inner.increment_counter(&self.prefixed(k), delta)
    }
    fn delete_counter(&mut self, k: &[u8]) {
        self.inner.delete_counter(&self.prefixed(k))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::engine::mem::{MemBatch, MemEngine};

    const P: &[u8] = b"~p|";

    fn wrapped() -> PrefixedEngine<MemEngine> {
        PrefixedEngine::new(MemEngine::new(), P)
    }

    fn commit_batch(eng: &PrefixedEngine<MemEngine>, f: impl FnOnce(&mut PrefixedBatch<MemBatch>)) {
        let mut b = eng.batch();
        f(&mut b);
        b.commit().unwrap();
    }

    #[test]
    fn roundtrip_lands_under_prefix_in_every_cf() {
        let eng = wrapped();
        commit_batch(&eng, |b| {
            b.put(b"k", b"point");
            b.put_queue(b"k", b"queue");
            b.increment_counter(b"k", 7);
        });
        // visible through the wrapper at the logical key
        assert_eq!(eng.get(b"k").unwrap(), Some(b"point".to_vec()));
        assert_eq!(eng.get_queue(b"k").unwrap(), Some(b"queue".to_vec()));
        assert_eq!(eng.get_counter(b"k").unwrap(), 7);
        // physically stored under the prefix, not at the raw key
        assert_eq!(eng.inner().get(b"k").unwrap(), None);
        assert_eq!(eng.inner().get(b"~p|k").unwrap(), Some(b"point".to_vec()));
        assert_eq!(eng.inner().get_counter(b"k").unwrap(), 0);
        assert_eq!(eng.inner().get_counter(b"~p|k").unwrap(), 7);
    }

    #[test]
    fn scan_strips_only_the_callers_prefix_and_stays_in_bounds() {
        let eng = wrapped();
        commit_batch(&eng, |b| {
            b.put(b"px|a", b"1");
            b.put(b"px|b", b"2");
            b.put(b"py|x", b"other"); // sibling logical prefix must not leak
        });
        // a raw key that looks prefixed-ish must not leak into wrapped scans
        let mut raw = eng.inner().batch();
        raw.put(b"px|raw", b"unprefixed");
        raw.commit().unwrap();

        let got: Vec<_> = eng.scan_from(b"px|", b"").map(|r| r.unwrap()).collect();
        assert_eq!(
            got,
            vec![
                (b"a".to_vec(), b"1".to_vec()),
                (b"b".to_vec(), b"2".to_vec())
            ]
        );
    }

    #[test]
    fn inner_mut_writes_raw_keys_in_the_same_atomic_batch() {
        let eng = wrapped();
        commit_batch(&eng, |b| {
            b.put(b"ours", b"prefixed");
            b.inner_mut().put(b"theirs", b"raw");
        });
        assert_eq!(eng.get(b"ours").unwrap(), Some(b"prefixed".to_vec()));
        assert_eq!(eng.inner().get(b"theirs").unwrap(), Some(b"raw".to_vec()));
        // and neither crossed into the other's keyspace
        assert_eq!(eng.get(b"theirs").unwrap(), None);
        assert_eq!(eng.inner().get(b"ours").unwrap(), None);
    }

    #[test]
    fn distinct_prefixes_over_one_engine_are_disjoint() {
        let raw = MemEngine::new();
        let a = PrefixedEngine::new(raw.clone(), b"a|");
        let b = PrefixedEngine::new(raw, b"b|");

        let mut batch = a.batch();
        batch.put(b"k", b"from-a");
        batch.commit().unwrap();
        let mut batch = b.batch();
        batch.put(b"k", b"from-b");
        batch.commit().unwrap();

        assert_eq!(a.get(b"k").unwrap(), Some(b"from-a".to_vec()));
        assert_eq!(b.get(b"k").unwrap(), Some(b"from-b".to_vec()));
    }

    #[test]
    fn nesting_composes() {
        let outer = PrefixedEngine::new(wrapped(), b"in|");
        let mut batch = outer.batch();
        batch.put(b"k", b"v");
        batch.commit().unwrap();

        assert_eq!(outer.get(b"k").unwrap(), Some(b"v".to_vec()));
        // outer's prefix applies first, then the wrapped engine's own
        assert_eq!(
            outer.inner().inner().get(b"~p|in|k").unwrap(),
            Some(b"v".to_vec())
        );
    }
}
