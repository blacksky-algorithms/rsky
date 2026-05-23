use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

#[derive(Debug)]
pub enum GraphError {
    Other(String),
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for GraphError {}

/// Snapshot of bulk-load progress visible to API handlers so they can refuse
/// queries about creators whose outgoing edges haven't been processed yet.
/// Without this, partial graph state would silently produce wrong empty results.
#[derive(Debug, Default)]
pub struct LoadState {
    complete: AtomicBool,
    last_completed_creator: RwLock<String>,
}

impl LoadState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark the keyset bulk-load fully complete. After this all creators are loaded.
    pub fn mark_complete(&self) {
        self.complete.store(true, Ordering::SeqCst);
    }

    /// Record that the keyset has finished processing every row whose `creator`
    /// is at or below `did`. Called when the bulk-load observes the keyset
    /// transition from one creator to the next.
    pub fn record_creator_completed(&self, did: &str) {
        let mut w = self.last_completed_creator.write().unwrap();
        if did > w.as_str() {
            *w = did.to_owned();
        }
    }

    /// True if `did` is guaranteed to have its outgoing edges fully loaded.
    pub fn creator_loaded(&self, did: &str) -> bool {
        if self.complete.load(Ordering::SeqCst) {
            return true;
        }
        let r = self.last_completed_creator.read().unwrap();
        !r.is_empty() && did <= r.as_str()
    }

    pub fn is_complete(&self) -> bool {
        self.complete.load(Ordering::SeqCst)
    }

    pub fn last_completed(&self) -> String {
        self.last_completed_creator.read().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_state_says_nothing_loaded() {
        let s = LoadState::new();
        assert!(!s.creator_loaded("did:plc:abc"));
        assert!(!s.creator_loaded(""));
    }

    #[test]
    fn complete_means_everyone_loaded() {
        let s = LoadState::new();
        s.mark_complete();
        assert!(s.creator_loaded("did:plc:zzz"));
        assert!(s.creator_loaded("did:plc:aaa"));
    }

    #[test]
    fn loaded_iff_did_le_last_completed() {
        let s = LoadState::new();
        s.record_creator_completed("did:plc:mmm");
        assert!(s.creator_loaded("did:plc:aaa"));
        assert!(s.creator_loaded("did:plc:mmm"));
        assert!(!s.creator_loaded("did:plc:nnn"));
        assert!(!s.creator_loaded("did:plc:zzz"));
    }

    #[test]
    fn record_does_not_go_backwards() {
        let s = LoadState::new();
        s.record_creator_completed("did:plc:mmm");
        s.record_creator_completed("did:plc:aaa"); // earlier alphabetically -- ignored
        assert_eq!(s.last_completed(), "did:plc:mmm");
        assert!(s.creator_loaded("did:plc:lll"));
    }
}
