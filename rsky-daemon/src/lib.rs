//! `rsky-daemon` — the permissioned-data syncer daemon.
//!
//! A syncer keeps its own copy of a space by pulling each member's permissioned
//! repo directly from that member's PDS (there is no relay), verifying the
//! deniable signed commit, and indexing the records for downstream readers (the
//! Blacksky appview reads this index). It is a dedicated service — NOT the
//! appview — because permissioned data is a parallel protocol.
//!
//! - [`engine`] — pure incremental sync: apply an oplog to the index, maintain
//!   the running [`rsky_space::LtHash`], authenticate against the signed
//!   commit, detect divergence.
//! - [`recovery`] — full-state recovery from a `getRepo` CAR when incremental
//!   sync cannot proceed.
//! - [`xrpc`] / [`repohost`] — HTTP clients for the space host and members'
//!   repo hosts.
//! - [`credentials`] — delegation-token → space-credential lifecycle.
//! - [`notify`] — the inbound `notifyWrite` / `notifySpaceDeleted` listener.
//! - [`runner`] — the loop composing all of the above.
//! - [`index`] / [`sqlite_index`] — the synced record index.

pub mod config;
pub mod credentials;
pub mod engine;
pub mod error;
pub mod index;
pub mod notify;
pub mod recovery;
pub mod repohost;
pub mod runner;
pub mod sqlite_index;
pub mod xrpc;

pub use credentials::{
    unix_now, CredentialProvider, CredentialSource, DelegationSource, PdsDelegationSource,
    StaticCredential,
};
pub use engine::{sync_repo, CommitKeyResolver, SyncOutcome};
pub use error::{DaemonError, Result};
pub use index::{InMemoryIndex, SpaceIndex};
pub use notify::{router as notify_router, NotifyState, WriteNotice};
pub use recovery::recover_repo;
pub use repohost::{HttpRepoHost, OplogPage, RepoHostClient};
pub use runner::{run, sync_repo_healing, sync_space_once, RunnerOptions, SweepReport};
pub use sqlite_index::{SpaceScopedIndex, SqliteIndex};
pub use xrpc::{HttpSpaceHost, SpaceHostClient};
