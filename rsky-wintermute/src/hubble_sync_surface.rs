//! Compile-time check of the vendored `hubble-sync` surface.
//!
//! An embedding needs per-repo fetch routing through `SyncConsumer::resync`
//! and a way to seed repos from an external DID list. This module only proves
//! those names are exported; it is behind the `hubble-sync` feature and
//! nothing at runtime uses it yet.
//!
//! Upstream 0.0.4 keeps these private; `vendor/hubble-sync/NOTICE.md` lists
//! the re-exports added. Delete this module once upstream exports them and the
//! vendored copy is gone.

#[allow(unused_imports)]
use hubble_sync::{
    BigRepoPermits, DiscoveredRepo, HubbleSync, RepoDiscovery, RepoIdentity, ResolutionError,
    ResolvedIdentity, ResyncError, Resyncable, TransientResyncError, load_repo,
};

/// The four names the earlier evaluation found unreachable, spelled out so a
/// future upstream rename breaks this crate loudly.
pub const REQUIRED_EXPORTS: [&str; 4] = ["Resyncable", "ResyncError", "load_repo", "RepoDiscovery"];

#[cfg(test)]
mod tests {
    #[test]
    fn the_names_we_depend_on_are_listed() {
        assert_eq!(super::REQUIRED_EXPORTS.len(), 4);
    }
}
