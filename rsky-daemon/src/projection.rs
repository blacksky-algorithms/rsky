//! Projection adapter contract. Delivery is driven by the durable journal,
//! never by writing straight through from the sync path.

use async_trait::async_trait;

use crate::error::Result;
use crate::router::SyncEvent;

/// Where a synced batch is sent.
#[async_trait]
pub trait Projector: Send + Sync {
    fn name(&self) -> &'static str;

    /// Deliver the events of one batch. Must be idempotent: a retry after a
    /// crash replays the same events.
    async fn project(&self, did: &str, rev: &str, events: &[SyncEvent]) -> Result<()>;
}
