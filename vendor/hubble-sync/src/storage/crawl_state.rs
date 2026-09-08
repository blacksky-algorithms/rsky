//! keyspace under hubble-sync's for per-strategy crawl state
//!
//! different crawl strategies pick a unique self-prefix, and everything gets
//! written under that (with the top-level strategy prefix), so things stay
//! contained and out of the way from consumer apps.
//!
//! "cs|" || <strategy-prefix> || NUL || <strategy-key> => <stragegy value>

use super::engine::{Pair, StorageBatch};
use super::{PREFIX_CRAWL_STATE, StorageEngine};

#[derive(Debug, Clone)]
pub struct CrawlState<S: StorageEngine> {
    storage: S,
    prefix: Vec<u8>,
}

impl<S: StorageEngine> CrawlState<S> {
    pub(crate) fn new(storage: S, strategy_id: &str) -> Self {
        let strategy_id_bytes = strategy_id.as_bytes();
        assert!(
            !strategy_id_bytes.contains(&0x00),
            "null bytes not allowed in strategy id"
        );

        let mut prefix = Vec::with_capacity(PREFIX_CRAWL_STATE.len() + strategy_id_bytes.len() + 1);
        prefix.extend_from_slice(PREFIX_CRAWL_STATE);
        prefix.extend_from_slice(strategy_id_bytes);
        prefix.push(0x00);

        Self { storage, prefix }
    }

    /// get a storage batch
    ///
    /// crawlers *must not* use the batch put/delete/etc apis directly, only the
    /// crawl state apis (which accept a Batch);
    pub fn batch(&self) -> S::Batch {
        self.storage.batch()
    }

    fn full_key(&self, strategy_key: &[u8]) -> Vec<u8> {
        let mut k = Vec::with_capacity(self.prefix.len() + strategy_key.len());
        k.extend_from_slice(&self.prefix);
        k.extend_from_slice(strategy_key);
        k
    }

    /// get a strategy state value by strategy key
    ///
    /// blocking: caller must wrap with spawn_blocking
    ///
    /// the full key (with strategy prefix) is expanded for you
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, S::Error> {
        self.storage.get(&self.full_key(key))
    }

    /// write a strategy key-value pair into a batch
    ///
    /// the full key (with strategy prefix) is expanded for you
    pub fn put(&self, key: &[u8], val: &[u8], batch: &mut S::Batch) {
        batch.put(&self.full_key(key), val);
    }

    /// delete a strategy key into a batch
    ///
    /// the full key (with strategy prefix) is expanded for you
    pub fn delete(&self, key: &[u8], batch: &mut S::Batch) {
        batch.delete(&self.full_key(key));
    }

    /// scan keys within this strategy
    ///
    /// blocking: caller must deal with spawn_blocking wrapping
    ///
    /// the full key (with strategy prefix) is expanded for you
    pub fn scan(
        &self,
        prefix: &[u8],
        from_suffix: &[u8],
    ) -> Box<dyn Iterator<Item = Result<Pair, S::Error>> + '_> {
        self.storage.scan_from(&self.full_key(prefix), from_suffix)
    }
}
