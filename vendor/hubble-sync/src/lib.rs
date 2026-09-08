mod cancel;
mod cid;
mod commit;
mod config;
mod crawl_strategy_list_repos;
mod firehose;
mod host;
mod hubble_sync;
mod identity;
mod metrics;
mod pending_identity_scheduler;
mod repo_actor;
mod repo_slot;
mod rescope_sweep;
mod resync;
mod resync_scheduler;
mod storage;
mod sync_consumer;
pub mod sync_handle;
mod tid;

// internal re-exports

use cancel::CancelExt;
use host::Hostname;

// public re-exports (TODO: needs cleaning up)

pub use cid::{Cid as DaslCid, CidParseError}; // VENDORED for now
pub use commit::{Commit, CommitObject, Op, OpKind};
pub use config::{DeepCrawl, SensitiveToken, SyncConfig, SyncScope, UpstreamConfig, UpstreamKind};
pub use firehose::{
    FirehoseAck, FirehoseConfig, FirehoseError, FirehoseEvent, FirehosePayload, FirehoseSubscriber,
};
pub use host::{Host, HostRegistry, HostRegistryConfig, HostRequestError, Sync11State};
pub use hubble_sync::{HubbleSync, HubbleSyncError};
pub use identity::{Did, DidMethod, HubbleSyncResolver, Resolve, SignatureError, SigningKey, did};
pub use identity::{ResolutionError, ResolvedIdentity};
pub use metrics::{
    BIG_REPO_WAIT_SECONDS_BUCKETS, REPO_ACTOR_ALIVE_SECONDS_BUCKETS, RESYNC_COUNT_BUCKETS,
    RESYNC_DURATION_SECONDS_BUCKETS, RESYNC_PHASE_SECONDS_BUCKETS, RESYNC_SIZE_BUCKETS,
    describe_metrics, histogram_buckets,
};
pub use pending_identity_scheduler::{DiscoveredRepo, PendingScheduler, RepoDiscovery};
pub use repo_actor::{
    ModerateOutcome, RepoContext, RepoMessage, RepoRegistry, RepoSendError, RepoSender,
    ResyncContext,
};
pub use repo_slot::{RepoSlot, RepoSlots, Slot, SlotDecodeError};
pub use resync::{
    BigRepoPermits, ResyncData, ResyncError, Resyncable, TransientResyncError, load_repo,
};
pub use resync_scheduler::ResyncScheduler;
pub use storage::engine::{
    PrefixedBatch, PrefixedEngine, StorageBatch, StorageEngine, StorageError,
};
pub use storage::repo::RepoIdentity;
pub use storage::repo::{
    AccountStatus, AccountStatusEvent, AccountStatusKind, DesyncReason, DesyncReasonKind,
    Desynchronized, ModAction, Repo, RepoCountsByState, RepoCountsByStatus, ResyncInfo, SyncStatus,
};
pub use storage::{CrawlState, LoadError, ModerateEvent};
pub use sync_consumer::{
    AppResult, ConsumerAppError, SimpleSyncConsumer, StatefulSyncConsumer, SyncConsumer,
};
pub use sync_handle::{RepoView, SyncHandle};
pub use tid::{Tid, tid_atproto};
