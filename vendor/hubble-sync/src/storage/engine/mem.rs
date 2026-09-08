//! in-memory StorageEngine (for tests etc).
//!
//! two `BTreeMap`s (one per CF) behind a single `Mutex`, to act as the two CFs/
//! partitions. batches collect ops, replay them under the lock on commit.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use super::prefixed::PrefixedEngine;
use super::types::{StorageBatch, StorageEngine, StorageError};

type Pair = (Vec<u8>, Vec<u8>);

/// prefix for tests exercising hubble-sync internals through a
/// [`PrefixedEngine`] -- deliberately not the default, to catch any
/// hardcoded-prefix assumptions
pub(crate) const TEST_PREFIX: &[u8] = b"~test|";

#[derive(Debug, Clone, Default)]
pub struct MemEngine {
    inner: Arc<Mutex<MemState>>,
}

#[derive(Debug, Default)]
struct MemState {
    point: BTreeMap<Vec<u8>, Vec<u8>>,
    queue: BTreeMap<Vec<u8>, Vec<u8>>,
    counter: BTreeMap<Vec<u8>, i64>,
}

/// in-memory ops can't fail
#[derive(Debug, thiserror::Error)]
pub enum MemEngineError {}

impl StorageError for MemEngineError {}

impl MemEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// a fresh engine wrapped under [`TEST_PREFIX`], the way hubble-sync
    /// internals always see storage
    pub(crate) fn new_prefixed() -> PrefixedEngine<MemEngine> {
        PrefixedEngine::new(Self::new(), TEST_PREFIX)
    }
}

impl StorageEngine for MemEngine {
    type Error = MemEngineError;
    type Batch = MemBatch;

    fn batch(&self) -> Self::Batch {
        MemBatch {
            state: Arc::clone(&self.inner),
            ops: Vec::new(),
        }
    }

    fn get(&self, k: &[u8]) -> Result<Option<Vec<u8>>, Self::Error> {
        let st = self.inner.lock().expect("mem state lock");
        Ok(st.point.get(k).cloned())
    }

    fn get_queue(&self, k: &[u8]) -> Result<Option<Vec<u8>>, Self::Error> {
        let st = self.inner.lock().expect("mem state lock");
        Ok(st.queue.get(k).cloned())
    }

    fn get_counter(&self, k: &[u8]) -> Result<i64, Self::Error> {
        let st = self.inner.lock().expect("mem state lock");
        Ok(st.counter.get(k).copied().unwrap_or(0))
    }

    fn scan_from(
        &self,
        prefix: &[u8],
        from_suffix: &[u8],
    ) -> Box<dyn Iterator<Item = Result<Pair, Self::Error>> + '_> {
        let mut start = prefix.to_vec();
        start.extend_from_slice(from_suffix);
        // collecting (all under the lock) is like an lsm's snapshotted iter
        let collected: Vec<Pair> = {
            self.inner
                .lock()
                .expect("mem state lock")
                .point
                .range(start..)
                .take_while(|(k, _)| k.starts_with(prefix)) // strip full prefix
                .map(|(k, v)| (k[prefix.len()..].to_vec(), v.clone()))
                .collect()
        };
        Box::new(collected.into_iter().map(Ok))
    }

    fn scan_from_queue(
        &self,
        prefix: &[u8],
        from_suffix: &[u8],
    ) -> Box<dyn Iterator<Item = Result<Pair, Self::Error>> + '_> {
        let mut start = prefix.to_vec();
        start.extend_from_slice(from_suffix);
        // collecting (all under the lock) is like an lsm's snapshotted iter
        let collected: Vec<Pair> = {
            self.inner
                .lock()
                .expect("mem state lock")
                .queue
                .range(start..)
                .take_while(|(k, _)| k.starts_with(prefix)) // strip full prefix
                .map(|(k, v)| (k[prefix.len()..].to_vec(), v.clone()))
                .collect()
        };
        Box::new(collected.into_iter().map(Ok))
    }
}

#[derive(Debug)]
pub struct MemBatch {
    state: Arc<Mutex<MemState>>,
    ops: Vec<MemOp>,
}

#[derive(Debug)]
enum MemOp {
    PutPoint(Vec<u8>, Vec<u8>),
    DeletePoint(Vec<u8>),
    PutQueue(Vec<u8>, Vec<u8>),
    DeleteQueue(Vec<u8>),
    PutCounter(Vec<u8>, i64),
    DeleteCounter(Vec<u8>),
    IncrementCounter(Vec<u8>, i64),
}

impl StorageBatch<MemEngineError> for MemBatch {
    fn commit(self) -> Result<(), MemEngineError> {
        let mut st = self.state.lock().expect("mem state lock");
        for op in self.ops {
            match op {
                MemOp::PutPoint(k, v) => {
                    st.point.insert(k, v);
                }
                MemOp::DeletePoint(k) => {
                    st.point.remove(&k);
                }
                MemOp::PutQueue(k, v) => {
                    st.queue.insert(k, v);
                }
                MemOp::DeleteQueue(k) => {
                    st.queue.remove(&k);
                }
                MemOp::PutCounter(k, n) => {
                    st.counter.insert(k, n);
                }
                MemOp::DeleteCounter(k) => {
                    st.counter.remove(&k);
                }
                MemOp::IncrementCounter(k, d) => {
                    *st.counter.entry(k).or_insert(0) += d;
                }
            }
        }
        Ok(())
    }

    fn put(&mut self, k: &[u8], v: &[u8]) {
        self.ops.push(MemOp::PutPoint(k.to_vec(), v.to_vec()));
    }
    fn delete(&mut self, k: &[u8]) {
        self.ops.push(MemOp::DeletePoint(k.to_vec()));
    }
    fn put_queue(&mut self, k: &[u8], v: &[u8]) {
        self.ops.push(MemOp::PutQueue(k.to_vec(), v.to_vec()));
    }
    fn delete_queue(&mut self, k: &[u8]) {
        self.ops.push(MemOp::DeleteQueue(k.to_vec()));
    }
    fn put_counter(&mut self, k: &[u8], n: i64) {
        self.ops.push(MemOp::PutCounter(k.to_vec(), n));
    }
    fn delete_counter(&mut self, k: &[u8]) {
        self.ops.push(MemOp::DeleteCounter(k.to_vec()));
    }
    fn increment_counter(&mut self, k: &[u8], d: i64) {
        self.ops.push(MemOp::IncrementCounter(k.to_vec(), d));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_point(eng: &MemEngine, k: &[u8], v: &[u8]) {
        let mut b = eng.batch();
        b.put(k, v);
        b.commit().unwrap();
    }

    fn scan_point(eng: &MemEngine, prefix: &[u8], from: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
        eng.scan_from(prefix, from).map(|r| r.unwrap()).collect()
    }

    #[test]
    fn scan_from_strips_prefix_and_orders() {
        let eng = MemEngine::new();
        put_point(&eng, b"px|c", b"3");
        put_point(&eng, b"px|a", b"1");
        put_point(&eng, b"px|b", b"2");
        assert_eq!(
            scan_point(&eng, b"px|", b""),
            vec![
                (b"a".to_vec(), b"1".to_vec()),
                (b"b".to_vec(), b"2".to_vec()),
                (b"c".to_vec(), b"3".to_vec()),
            ]
        );
    }

    #[test]
    fn scan_from_stops_at_prefix_boundary() {
        let eng = MemEngine::new();
        put_point(&eng, b"px|a", b"1");
        put_point(&eng, b"py|a", b"other"); // adjacent prefix must not leak in
        assert_eq!(
            scan_point(&eng, b"px|", b""),
            vec![(b"a".to_vec(), b"1".to_vec())]
        );
    }

    #[test]
    fn scan_from_resumes_at_suffix_inclusive() {
        let eng = MemEngine::new();
        for k in [b"px|a".as_slice(), b"px|b", b"px|c"] {
            put_point(&eng, k, b"v");
        }
        let got: Vec<_> = eng.scan_from(b"px|", b"b").map(|r| r.unwrap().0).collect();
        assert_eq!(
            got,
            vec![b"b".to_vec(), b"c".to_vec()],
            "from_suffix is an inclusive lower bound"
        );
    }

    #[test]
    fn scan_from_empty_yields_nothing() {
        let eng = MemEngine::new();
        assert!(scan_point(&eng, b"px|", b"").is_empty());
    }

    #[test]
    fn point_and_queue_are_separate_namespaces() {
        let eng = MemEngine::new();
        let mut b = eng.batch();
        b.put(b"k", b"point");
        b.put_queue(b"k", b"queue");
        b.commit().unwrap();
        assert_eq!(eng.get(b"k").unwrap(), Some(b"point".to_vec()));
        assert_eq!(eng.get_queue(b"k").unwrap(), Some(b"queue".to_vec()));
        let pt: Vec<_> = eng.scan_from(b"", b"").map(|r| r.unwrap()).collect();
        let qu: Vec<_> = eng.scan_from_queue(b"", b"").map(|r| r.unwrap()).collect();
        assert_eq!(pt, vec![(b"k".to_vec(), b"point".to_vec())]);
        assert_eq!(qu, vec![(b"k".to_vec(), b"queue".to_vec())]);
    }

    // --- counter CF ---

    fn commit_batch(eng: &MemEngine, f: impl FnOnce(&mut MemBatch)) {
        let mut b = eng.batch();
        f(&mut b);
        b.commit().unwrap();
    }

    #[test]
    fn get_counter_is_zero_when_unset() {
        let eng = MemEngine::new();
        assert_eq!(eng.get_counter(b"c|repos|synchronized").unwrap(), 0);
    }

    #[test]
    fn put_counter_sets_an_absolute_value() {
        let eng = MemEngine::new();
        commit_batch(&eng, |b| b.put_counter(b"k", 100));
        assert_eq!(eng.get_counter(b"k").unwrap(), 100);
        // put overwrites, it doesn't accumulate.
        commit_batch(&eng, |b| b.put_counter(b"k", 7));
        assert_eq!(eng.get_counter(b"k").unwrap(), 7);
    }

    #[test]
    fn delete_counter_resets_to_zero() {
        let eng = MemEngine::new();
        commit_batch(&eng, |b| b.put_counter(b"k", 42));
        commit_batch(&eng, |b| b.delete_counter(b"k"));
        assert_eq!(eng.get_counter(b"k").unwrap(), 0);
    }

    #[test]
    fn increments_accumulate_across_batches() {
        let eng = MemEngine::new();
        commit_batch(&eng, |b| b.increment_counter(b"k", 5));
        commit_batch(&eng, |b| b.increment_counter(b"k", 3));
        assert_eq!(eng.get_counter(b"k").unwrap(), 8);
    }

    #[test]
    fn increments_within_one_batch_accumulate() {
        let eng = MemEngine::new();
        commit_batch(&eng, |b| {
            b.increment_counter(b"k", 10);
            b.increment_counter(b"k", -4);
            b.increment_counter(b"k", 1);
        });
        assert_eq!(eng.get_counter(b"k").unwrap(), 7);
    }

    #[test]
    fn increment_builds_on_a_put() {
        let eng = MemEngine::new();
        commit_batch(&eng, |b| {
            b.put_counter(b"k", 10);
            b.increment_counter(b"k", 5);
        });
        assert_eq!(eng.get_counter(b"k").unwrap(), 15);
    }

    #[test]
    fn counter_ops_are_atomic_with_their_batch() {
        let eng = MemEngine::new();
        let mut b = eng.batch();
        b.increment_counter(b"k", 42);
        assert_eq!(
            eng.get_counter(b"k").unwrap(),
            0,
            "an uncommitted increment is not visible"
        );
        b.commit().unwrap();
        assert_eq!(eng.get_counter(b"k").unwrap(), 42);
    }

    #[test]
    fn counters_are_a_separate_namespace() {
        let eng = MemEngine::new();
        commit_batch(&eng, |b| {
            b.put(b"k", b"point");
            b.put_queue(b"k", b"queue");
            b.increment_counter(b"k", 9);
        });
        assert_eq!(eng.get(b"k").unwrap(), Some(b"point".to_vec()));
        assert_eq!(eng.get_queue(b"k").unwrap(), Some(b"queue".to_vec()));
        assert_eq!(eng.get_counter(b"k").unwrap(), 9);
    }

    #[test]
    #[should_panic]
    fn increment_counter_overflow_panics() {
        // checked_add + unwrap means overflow is a hard failure, not a wrap.
        let eng = MemEngine::new();
        commit_batch(&eng, |b| b.put_counter(b"k", i64::MAX));
        commit_batch(&eng, |b| b.increment_counter(b"k", 1));
    }
}
