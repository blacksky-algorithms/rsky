use dashmap::DashMap;
use roaring::RoaringBitmap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::bloom;

pub struct FollowGraph {
    // DID <-> UID bidirectional mapping
    pub did_to_uid: DashMap<String, u32>,
    pub uid_to_did: DashMap<u32, String>,
    next_uid: AtomicU32,

    // Per-user bitmaps
    pub followers: DashMap<u32, RoaringBitmap>,
    pub following: DashMap<u32, RoaringBitmap>,

    // Per-user bloom filter of followers for fast rejection
    pub follower_blooms: DashMap<u32, bloomfilter::Bloom<u32>>,

    // (actor_uid, rkey) -> subject_uid. The firehose delete event for a follow
    // record carries only the rkey, not the subject DID, so to remove a follow
    // we must remember which subject the rkey pointed at when it was created.
    // Populated by add_follow_with_rkey on firehose creates and (optionally) by
    // a 3-column bulk-load CSV. Not persisted -- we accept that pre-snapshot
    // follows are not reversible until the next snapshot rebuild.
    pub follow_rkeys: DashMap<(u32, String), u32>,

    // Stats
    follow_count: AtomicU64,
}

impl FollowGraph {
    pub fn new() -> Self {
        Self {
            did_to_uid: DashMap::new(),
            uid_to_did: DashMap::new(),
            next_uid: AtomicU32::new(1),
            followers: DashMap::new(),
            following: DashMap::new(),
            follower_blooms: DashMap::new(),
            follow_rkeys: DashMap::new(),
            follow_count: AtomicU64::new(0),
        }
    }

    /// Get or assign a UID for a DID.
    pub fn get_or_assign_uid(&self, did: &str) -> u32 {
        if let Some(uid) = self.did_to_uid.get(did) {
            return *uid;
        }
        let uid = self.next_uid.fetch_add(1, Ordering::Relaxed);
        self.did_to_uid.insert(did.to_owned(), uid);
        self.uid_to_did.insert(uid, did.to_owned());
        uid
    }

    /// Get UID for a DID without assigning.
    pub fn get_uid(&self, did: &str) -> Option<u32> {
        self.did_to_uid.get(did).map(|v| *v)
    }

    /// Get DID for a UID.
    pub fn get_did(&self, uid: u32) -> Option<String> {
        self.uid_to_did.get(&uid).map(|v| v.clone())
    }

    /// Add a follow relationship: actor follows subject.
    pub fn add_follow(&self, actor_did: &str, subject_did: &str) {
        let actor_uid = self.get_or_assign_uid(actor_did);
        let subject_uid = self.get_or_assign_uid(subject_did);

        // Update following bitmap for actor
        self.following
            .entry(actor_uid)
            .or_insert_with(RoaringBitmap::new)
            .insert(subject_uid);

        // Update followers bitmap for subject
        self.followers
            .entry(subject_uid)
            .or_insert_with(RoaringBitmap::new)
            .insert(actor_uid);

        // Update bloom filter for subject's followers
        self.follower_blooms
            .entry(subject_uid)
            .or_insert_with(|| bloom::new_bloom_filter(100))
            .set(&actor_uid);

        self.follow_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Add a follow relationship and remember the rkey so a later firehose
    /// delete event (which carries only the rkey) can resolve back to the subject.
    pub fn add_follow_with_rkey(&self, actor_did: &str, rkey: &str, subject_did: &str) {
        self.add_follow(actor_did, subject_did);
        let actor_uid = self.get_or_assign_uid(actor_did);
        let subject_uid = self.get_or_assign_uid(subject_did);
        self.follow_rkeys
            .insert((actor_uid, rkey.to_owned()), subject_uid);
    }

    /// Remove a follow by rkey, looking up the subject we recorded at create time.
    /// Returns true if the follow was known and removed; false if the rkey was
    /// never seen (e.g. follow predates rsky-graph's rkey index).
    pub fn remove_follow_by_rkey(&self, actor_did: &str, rkey: &str) -> bool {
        let Some(actor_uid) = self.get_uid(actor_did) else {
            return false;
        };
        let Some((_, subject_uid)) = self.follow_rkeys.remove(&(actor_uid, rkey.to_owned())) else {
            return false;
        };
        if let Some(mut bm) = self.following.get_mut(&actor_uid) {
            bm.remove(subject_uid);
        }
        if let Some(mut bm) = self.followers.get_mut(&subject_uid) {
            bm.remove(actor_uid);
        }
        self.follow_count.fetch_sub(1, Ordering::Relaxed);
        true
    }

    /// Remove a follow relationship: actor unfollows subject.
    pub fn remove_follow(&self, actor_did: &str, subject_did: &str) {
        let Some(actor_uid) = self.get_uid(actor_did) else {
            return;
        };
        let Some(subject_uid) = self.get_uid(subject_did) else {
            return;
        };

        if let Some(mut bm) = self.following.get_mut(&actor_uid) {
            bm.remove(subject_uid);
        }
        if let Some(mut bm) = self.followers.get_mut(&subject_uid) {
            bm.remove(actor_uid);
        }

        // Bloom filters don't support removal -- rebuild periodically
        // For now, the bloom may have false positives for removed follows,
        // which is acceptable (it just means we do the bitmap check unnecessarily)

        self.follow_count.fetch_sub(1, Ordering::Relaxed);
    }

    /// Find mutual follows: people the viewer follows who also follow the target.
    /// This is the hot path -- must be sub-millisecond.
    pub fn get_follows_following(&self, viewer_did: &str, target_did: &str) -> Vec<String> {
        let Some(viewer_uid) = self.get_uid(viewer_did) else {
            return vec![];
        };
        let Some(target_uid) = self.get_uid(target_did) else {
            return vec![];
        };

        let Some(viewer_following) = self.following.get(&viewer_uid) else {
            return vec![];
        };

        // Layer 1: Bloom filter fast rejection
        // If none of the viewer's following set could possibly be in target's followers,
        // skip the bitmap intersection entirely.
        if let Some(bloom) = self.follower_blooms.get(&target_uid) {
            let mut any_maybe = false;
            for uid in viewer_following.iter() {
                if bloom.check(&uid) {
                    any_maybe = true;
                    break;
                }
            }
            if !any_maybe {
                crate::metrics::GRAPH_BLOOM_REJECTIONS.inc();
                return vec![];
            }
        }

        // Layer 2: Roaring bitmap intersection
        let Some(target_followers) = self.followers.get(&target_uid) else {
            return vec![];
        };

        let intersection = viewer_following.value() & target_followers.value();

        intersection
            .iter()
            .filter_map(|uid| self.get_did(uid))
            .collect()
    }

    /// Check if actor follows subject.
    pub fn is_following(&self, actor_did: &str, subject_did: &str) -> bool {
        let Some(actor_uid) = self.get_uid(actor_did) else {
            return false;
        };
        let Some(subject_uid) = self.get_uid(subject_did) else {
            return false;
        };
        self.following
            .get(&actor_uid)
            .map_or(false, |bm| bm.contains(subject_uid))
    }

    pub fn user_count(&self) -> usize {
        self.did_to_uid.len()
    }

    pub fn follow_count(&self) -> u64 {
        self.follow_count.load(Ordering::Relaxed)
    }

    pub fn next_uid(&self) -> u32 {
        self.next_uid.load(Ordering::Relaxed)
    }

    pub fn set_next_uid(&self, uid: u32) {
        self.next_uid.store(uid, Ordering::Relaxed);
    }
}
