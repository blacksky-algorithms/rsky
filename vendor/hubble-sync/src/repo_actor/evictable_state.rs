//! EvictableState: registry/actor eviction negotiation

use std::sync::atomic::{AtomicU64, Ordering};

/// packed shared state between registry and actors
///
/// - top 32 bits: `send_count` incremented by registry
/// - bottom 32: `unloadable_at` stamped by the actor when it has no work
///
/// unload is safe if `send_count == unloadable_at`
///
/// TODO: could we be clever and fetch_add dt<<32 to make `send` a shared clock?
///
/// i don't *think* wrapping matters, since the count side just updates to
/// whatever the send side is.
#[derive(Debug)]
pub struct EvictableState(AtomicU64);

impl EvictableState {
    fn pack(send: u32, at: u32) -> u64 {
        (send as u64) << 32 | at as u64
    }
    fn unpack(packed: u64) -> (u32, u32) {
        let send = (packed >> 32) as u32;
        let at = packed as u32; // cast drops high bits
        (send, at)
    }
    fn unpack_send(packed: u64) -> u32 {
        (packed >> 32) as u32
    }
    pub fn new() -> Self {
        let send: u32 = 0;
        let at: u32 = send.wrapping_sub(1); // intentionally mismatched
        Self(AtomicU64::new(Self::pack(send, at)))
    }
    pub fn registry_note_send(&self) {
        let send_1 = Self::pack(1, 0);
        self.0.fetch_add(send_1, Ordering::Release);
    }
    pub fn registry_check_evictable(&self) -> bool {
        let (send, at) = Self::unpack(self.0.load(Ordering::Acquire));
        at == send
    }
    pub fn actor_snapshot(&self) -> u64 {
        self.0.load(Ordering::Acquire)
    }
    /// fails (returns false) if the registry hasn't sent since snapshot
    pub fn actor_make_evictable_since(&self, since_snapshot: u64) -> bool {
        let send = Self::unpack_send(since_snapshot);
        let at = send;
        let evicatable = Self::pack(send, at);
        self.0
            .compare_exchange(
                since_snapshot,
                evicatable,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_state_is_not_evictable() {
        // `new()` intentionally mismatches send and at so a brand-new
        // actor (which hasn't had a chance to claim yet) is never
        // considered evictable.
        let s = EvictableState::new();
        assert!(!s.registry_check_evictable());
    }

    #[test]
    fn actor_claims_evictable_from_fresh_snapshot() {
        let s = EvictableState::new();
        let snap = s.actor_snapshot();
        assert!(s.actor_make_evictable_since(snap));
        assert!(s.registry_check_evictable());
    }

    #[test]
    fn note_send_invalidates_an_existing_evictable_claim() {
        let s = EvictableState::new();
        let snap = s.actor_snapshot();
        assert!(s.actor_make_evictable_since(snap));
        assert!(s.registry_check_evictable());

        s.registry_note_send();
        assert!(!s.registry_check_evictable());
    }

    #[test]
    fn cas_rejects_stale_snapshot_after_send() {
        // Actor snapshots, registry sends between snapshot and CAS, CAS
        // must fail — this is what prevents an evictable claim from
        // landing while an event is in flight.
        let s = EvictableState::new();
        let snap = s.actor_snapshot();
        s.registry_note_send();

        assert!(!s.actor_make_evictable_since(snap));
        assert!(!s.registry_check_evictable());
    }

    #[test]
    fn actor_can_reclaim_evictable_with_fresh_snapshot_after_send() {
        let s = EvictableState::new();
        // first claim
        let snap1 = s.actor_snapshot();
        assert!(s.actor_make_evictable_since(snap1));
        // send invalidates
        s.registry_note_send();
        assert!(!s.registry_check_evictable());
        // re-snapshot post-send, re-claim succeeds
        let snap2 = s.actor_snapshot();
        assert!(s.actor_make_evictable_since(snap2));
        assert!(s.registry_check_evictable());
    }

    #[test]
    fn consecutive_sends_each_invalidate_prior_snapshots() {
        // each `registry_note_send` produces a state distinct from any
        // earlier snapshot — so stale snapshots cannot succeed CAS even
        // across multiple sends without a corresponding claim.
        let s = EvictableState::new();
        let snap1 = s.actor_snapshot();
        s.registry_note_send();
        let snap2 = s.actor_snapshot();
        s.registry_note_send();

        assert_ne!(snap1, snap2);
        assert!(!s.actor_make_evictable_since(snap1));
        assert!(!s.actor_make_evictable_since(snap2));
        assert!(!s.registry_check_evictable());
    }

    #[test]
    fn send_after_claim_unmarks_evictability() {
        // Mirror of the registry-side send-order race fix at the
        // state-machine level: even if the actor's CAS lands while a
        // send is "mid-flight" in the registry, the trailing
        // `note_send` is guaranteed to un-mark the actor by the time
        // eviction observes the state.
        let s = EvictableState::new();

        // actor claims evictable
        let snap = s.actor_snapshot();
        assert!(s.actor_make_evictable_since(snap));
        assert!(s.registry_check_evictable());

        // trailing note_send (from a send whose tx.try_send already
        // landed) flips state back to non-evictable
        s.registry_note_send();
        assert!(!s.registry_check_evictable());
    }
}
