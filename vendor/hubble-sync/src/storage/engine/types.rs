//! Shared trait stuff for key-value storage backends
//!
//! generally blocking (must be run inside tokio::spawn_blocking). byte-oriented
//! and partition-aware
//!
//! per-repo operations are serialized by the Repository actor, so snapshots are
//! usually not needed.

use std::time::Duration;

pub(crate) type Pair = (Vec<u8>, Vec<u8>);

pub trait StorageError: std::error::Error + Send + Sync + 'static {}

/// Core concrete storage trait
///
/// we need a general point-oriented column family (/partition for fjall) and a
/// queue-oriented one.
pub trait StorageEngine: Clone + Send + Sync + 'static {
    type Error: StorageError;
    type Batch: StorageBatch<Self::Error> + Send;

    fn batch(&self) -> Self::Batch;

    /// point-CF get
    fn get(&self, k: &[u8]) -> Result<Option<Vec<u8>>, Self::Error>;
    /// queue-CF get
    fn get_queue(&self, k: &[u8]) -> Result<Option<Vec<u8>>, Self::Error>;
    /// counter-CF get
    fn get_counter(&self, k: &[u8]) -> Result<i64, Self::Error>;

    /// point-CF scan (always within a prefix)
    ///
    /// trims the iterated keys' prefix! including the provided prefix!
    fn scan_from(
        &self,
        prefix: &[u8],
        from_suffix: &[u8],
    ) -> Box<dyn Iterator<Item = Result<Pair, Self::Error>> + '_>;

    /// queue-CF scan (always within a prefix)
    ///
    /// trims the iterated keys' prefix! including the provided prefix!
    fn scan_from_queue(
        &self,
        prefix: &[u8],
        from_suffix: &[u8],
    ) -> Box<dyn Iterator<Item = Result<Pair, Self::Error>> + '_>;

    /// optional periodic task the engine wants to hook into
    ///
    /// called by the hubble-sync runtime
    ///
    /// maintenance returning None unschedules the task (will not be restarted)
    ///
    /// returning Ok(d) is a *request* to be called again after `d` time, which
    /// the runtime will try to respect (but offers no guarantees).
    fn maintenance(&self) -> Result<Option<Duration>, Self::Error> {
        Ok(None)
    }

    /// optional engine metrics update
    ///
    /// for gauges which need to be driven on an interval. called periodically
    /// by the runtimes's own polled-gauge driver.
    ///
    /// default: no-op.
    fn report_metrics(&self) {}
}

pub trait StorageBatch<E: StorageError>: Send {
    fn commit(self) -> Result<(), E>;
    /// point-CF put
    fn put(&mut self, k: &[u8], v: &[u8]);
    /// point-CF delete
    fn delete(&mut self, k: &[u8]);
    /// queue-CF put
    fn put_queue(&mut self, k: &[u8], v: &[u8]);
    /// queue-CF delete
    fn delete_queue(&mut self, k: &[u8]);
    /// counter-CF put
    fn put_counter(&mut self, k: &[u8], count: i64);
    /// counter-CF increment
    fn increment_counter(&mut self, k: &[u8], delta: i64);
    /// counter-CF delete
    fn delete_counter(&mut self, k: &[u8]);
}
