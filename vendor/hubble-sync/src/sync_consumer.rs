//! SyncConsumer trait (the app's hook surface) and public-facing stuff

use crate::commit::Commit;
use crate::resync::{ResyncData, ResyncError, Resyncable};
use crate::{
    AccountStatus, RepoContext, RepoSlot, RepoSlots, ResyncContext, StorageEngine, StorageError,
};

/// Errors that consumer app callbacks are allowed to produce
///
/// this is intentionally strict. if you tell hubble-sync ther was a problem, it
/// will take agressive action to address.
///
/// if you just have some fallible processing that shouldn't stop the world,
/// create your own catching-logging wrapper.
#[derive(Debug, thiserror::Error)]
pub enum ConsumerAppError<E: StorageError> {
    #[error("unrecoverable consumer app callback error: {reason}")]
    Fatal { reason: String },
    #[error("storage error: {0}")]
    Storage(#[from] E),
    #[error("consumer app callback result requires repo resync: {reason}")]
    Desynchronize { reason: String },
}

pub type AppResult<S, T> = Result<T, ConsumerAppError<<S as StorageEngine>::Error>>;

/// Implement this trait to hook into the hubble-sync machinery
///
/// This is the right place to get started! Consider the more advanced options
/// when you need:
///
/// - optimized reads for some (small!) per-repo state: [`StatefulSyncConsumer`]
/// - customized resync approaches (like lightrail): [`SyncConsumer`]
pub trait SimpleSyncConsumer: Send + Sync + 'static {
    type Engine: StorageEngine;

    /// verified firehose commit. changes put in batch are atomic with sync1.1 bookkeeping
    fn apply_commit(
        &self,
        commit: Commit,
        repo: &RepoContext,
        batch: &mut <Self::Engine as StorageEngine>::Batch,
    ) -> AppResult<Self::Engine, ()>;

    /// apply a resync (full-repo update)
    ///
    /// anything that needs to be atomically updated with the sync1.1 bookkeeping: put it in the batch
    fn apply_resync(
        &self,
        data: &mut ResyncData,
        repo: &RepoContext,
        batch: &mut <Self::Engine as StorageEngine>::Batch,
    ) -> AppResult<Self::Engine, ()>;

    /// account/identity/moderation status change
    fn apply_status(
        &self,
        prev_status: &AccountStatus,
        repo: &RepoContext,
        batch: &mut <Self::Engine as StorageEngine>::Batch,
    ) -> AppResult<Self::Engine, ()>;
}

/// Implement this trait for access to optimized per-repo state
///
/// The per-repo state:
/// - gets eagerly loaded when the repo actor wakes
/// - is cached on the actor, bypassing storage reads for already-awake repos
/// - is co-located with hubble-sync's own per-repo state for minimal read
///   amplification
///
/// Hubble-sync splits per-repo state across two different keys to reduce write
/// amplification: a minimal `prev` key holds the sync1.1 state needed to apply
/// inductive proofs to each new commit, and which gets updated on each commit.
/// An `info` key holds everything else, which updates on a slower cadence.
///
/// StatefulSyncConsumer offers two "slots", one for each of those keys:
///
/// **`CommitState`**: is stored next to a repo's hubble-sync `prev` key, and
/// works best for state which updates at a similar rate to firehose commits.
/// For example: Hubble uses this slot to store each commit's signature.
///
/// **`InfoState`**: is stored next to a repo's hubble-sync `info` key, and
/// works for anything that updates at a slower cadence. For example: Hubble
/// uses this slot to store each resync's *generation marker prefix*, to non-
/// destructively resync large repos which don't fit in the main atomic resync
/// storage batch.
pub trait StatefulSyncConsumer: Send + Sync + 'static {
    type Engine: StorageEngine;
    type CommitState: RepoSlot;
    type InfoState: RepoSlot;

    /// verified firehose commit. changes put in batch are atomic with sync1.1 bookkeeping
    fn apply_commit(
        &self,
        commit: Commit,
        slots: &mut RepoSlots<'_, Self::CommitState, Self::InfoState>,
        repo: &RepoContext,
        batch: &mut <Self::Engine as StorageEngine>::Batch,
    ) -> AppResult<Self::Engine, ()>;

    /// apply a resync (full-repo update)
    ///
    /// anything that needs to be atomically updated with the sync1.1 bookkeeping: put it in the batch
    fn apply_resync(
        &self,
        data: &mut ResyncData,
        slots: &mut RepoSlots<'_, Self::CommitState, Self::InfoState>,
        repo: &RepoContext,
        batch: &mut <Self::Engine as StorageEngine>::Batch,
    ) -> AppResult<Self::Engine, ()>;

    /// account/identity/moderation status change
    fn apply_status(
        &self,
        prev_status: &AccountStatus,
        slots: &mut RepoSlots<'_, Self::CommitState, Self::InfoState>,
        repo: &RepoContext,
        batch: &mut <Self::Engine as StorageEngine>::Batch,
    ) -> AppResult<Self::Engine, ()>;
}

/// Implement this trait if you need custom resync data fetching
///
/// Most apps should be fine implementing the default [`SimpleSyncConsumer`].
pub trait SyncConsumer: Send + Sync + 'static {
    type Engine: StorageEngine;
    type ResyncData: Resyncable;
    type CommitState: RepoSlot;
    type InfoState: RepoSlot;

    /// get the resync dataset
    ///
    /// usually the default implementation is fine, unless you're doing weird things like lightrail
    fn resync(
        &self,
        slots: &mut RepoSlots<'_, Self::CommitState, Self::InfoState>,
        ctx: &mut ResyncContext,
    ) -> impl Future<Output = Result<Self::ResyncData, ResyncError>> + Send;

    /// verified firehose commit. changes put in batch are atomic with sync1.1 bookkeeping
    fn apply_commit(
        &self,
        commit: Commit,
        slots: &mut RepoSlots<'_, Self::CommitState, Self::InfoState>,
        repo: &RepoContext,
        batch: &mut <Self::Engine as StorageEngine>::Batch,
    ) -> AppResult<Self::Engine, ()>;

    /// apply a resync (full-repo update)
    ///
    /// anything that needs to be atomically updated with the sync1.1 bookkeeping: put it in the batch
    fn apply_resync(
        &self,
        data: &mut Self::ResyncData,
        slots: &mut RepoSlots<'_, Self::CommitState, Self::InfoState>,
        repo: &RepoContext,
        batch: &mut <Self::Engine as StorageEngine>::Batch,
    ) -> AppResult<Self::Engine, ()>;

    /// account/identity/moderation status change
    fn apply_status(
        &self,
        prev_status: &AccountStatus,
        slots: &mut RepoSlots<'_, Self::CommitState, Self::InfoState>,
        repo: &RepoContext,
        batch: &mut <Self::Engine as StorageEngine>::Batch,
    ) -> AppResult<Self::Engine, ()>;
}

impl<T: SimpleSyncConsumer> StatefulSyncConsumer for T {
    type Engine = T::Engine;
    type CommitState = ();
    type InfoState = ();

    fn apply_commit(
        &self,
        c: Commit,
        _s: &mut RepoSlots<'_, Self::CommitState, Self::InfoState>,
        r: &RepoContext,
        b: &mut <Self::Engine as StorageEngine>::Batch,
    ) -> AppResult<Self::Engine, ()> {
        T::apply_commit(self, c, r, b)
    }

    fn apply_resync(
        &self,
        d: &mut ResyncData,
        _s: &mut RepoSlots<'_, Self::CommitState, Self::InfoState>,
        r: &RepoContext,
        b: &mut <Self::Engine as StorageEngine>::Batch,
    ) -> AppResult<Self::Engine, ()> {
        T::apply_resync(self, d, r, b)
    }

    fn apply_status(
        &self,
        p: &AccountStatus,
        _s: &mut RepoSlots<'_, Self::CommitState, Self::InfoState>,
        r: &RepoContext,
        b: &mut <Self::Engine as StorageEngine>::Batch,
    ) -> AppResult<Self::Engine, ()> {
        T::apply_status(self, p, r, b)
    }
}

impl<T: StatefulSyncConsumer> SyncConsumer for T {
    type Engine = T::Engine;
    type CommitState = T::CommitState;
    type InfoState = T::InfoState;
    type ResyncData = ResyncData;

    fn apply_commit(
        &self,
        c: Commit,
        s: &mut RepoSlots<'_, Self::CommitState, Self::InfoState>,
        r: &RepoContext,
        b: &mut <Self::Engine as StorageEngine>::Batch,
    ) -> AppResult<Self::Engine, ()> {
        T::apply_commit(self, c, s, r, b)
    }

    fn apply_resync(
        &self,
        d: &mut Self::ResyncData,
        s: &mut RepoSlots<'_, Self::CommitState, Self::InfoState>,
        r: &RepoContext,
        b: &mut <Self::Engine as StorageEngine>::Batch,
    ) -> AppResult<Self::Engine, ()> {
        T::apply_resync(self, d, s, r, b)
    }

    fn apply_status(
        &self,
        p: &AccountStatus,
        s: &mut RepoSlots<'_, Self::CommitState, Self::InfoState>,
        r: &RepoContext,
        b: &mut <Self::Engine as StorageEngine>::Batch,
    ) -> AppResult<Self::Engine, ()> {
        T::apply_status(self, p, s, r, b)
    }

    async fn resync(
        &self,
        _slots: &mut RepoSlots<'_, Self::CommitState, Self::InfoState>,
        ctx: &mut ResyncContext<'_>,
    ) -> Result<ResyncData, ResyncError> {
        ctx.load_default().await
    }
}
