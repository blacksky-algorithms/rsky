//! Client for a member's permissioned repo host (their PDS).
//!
//! The live implementation calls `com.atproto.space.listRepoOps` / `getRepo`
//! with a space credential — those methods do not exist until upstream ships
//! permissioned data (PR #5187), so the HTTP client is a deferred stub. The
//! trait lets the [`crate::engine`] sync logic be written and tested now against
//! a mock.

use async_trait::async_trait;
use rsky_space::types::{RepoOp, SignedCommit};

use crate::error::{DaemonError, Result};

/// A page of the operation log, plus the current signed commit when the page
/// reaches the repo head (proposal §Incremental sync).
pub struct OplogPage {
    pub ops: Vec<RepoOp>,
    /// Present iff this page includes the last available op — lets the syncer
    /// authenticate the resulting state against the signed commit.
    pub commit: Option<SignedCommit>,
}

/// Reads permissioned repos from their hosts.
#[async_trait]
pub trait RepoHostClient: Send + Sync {
    /// `listRepoOps` since a revision (inlining record values by default).
    async fn list_repo_ops(&self, did: &str, since: Option<&str>) -> Result<OplogPage>;
}

/// Deferred HTTP client targeting `com.atproto.space.*` on a member's PDS.
pub struct HttpRepoHost {
    #[allow(dead_code)]
    credential: String,
}

impl HttpRepoHost {
    pub fn new(credential: String) -> Self {
        Self { credential }
    }
}

#[async_trait]
impl RepoHostClient for HttpRepoHost {
    async fn list_repo_ops(&self, _did: &str, _since: Option<&str>) -> Result<OplogPage> {
        // Blocked on upstream com.atproto.space.listRepoOps (PR #5187).
        Err(DaemonError::NotImplemented(
            "com.atproto.space.listRepoOps (upstream PR #5187)",
        ))
    }
}
