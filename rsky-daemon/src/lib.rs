//! `rsky-daemon` — the permissioned-data syncer daemon.
//!
//! A syncer keeps its own copy of a space by pulling each member's permissioned
//! repo directly from that member's PDS (there is no relay), verifying the
//! deniable signed commit, and indexing the records for downstream readers (the
//! Blacksky appview reads this index). It is a dedicated service — NOT the
//! appview — because permissioned data is a parallel protocol.
//!
//! The [`engine`] holds the pure sync logic (apply an oplog → index, maintain
//! the running [`rsky_space::LtHash`], authenticate against the signed commit,
//! detect divergence), abstracted over a [`repohost::RepoHostClient`] and a
//! [`index::SpaceIndex`]. It is fully tested against mocks now. The live
//! `com.atproto.space.listRepoOps`/`getRepo` HTTP client and the writer-set /
//! notification loop are gated on upstream (PR #5187); the daemon runs the
//! moment members' PDSes expose those methods.

pub mod config;
pub mod engine;
pub mod error;
pub mod index;
pub mod repohost;

pub use engine::{sync_repo, CommitKeyResolver, SyncOutcome};
pub use error::{DaemonError, Result};
pub use index::{InMemoryIndex, SpaceIndex};
pub use repohost::{OplogPage, RepoHostClient};
