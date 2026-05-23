use crate::graph::FollowGraph;
use bloomfilter::Bloom;

/// Create a new bloom filter sized for the expected number of items.
/// Uses 10 bits per element for ~1% false positive rate.
pub fn new_bloom_filter(expected_items: usize) -> Bloom<u32> {
    let items = expected_items.max(10);
    let bits = items * 10;
    let hashes = 7; // optimal for 10 bits/element
    Bloom::new(bits, hashes)
}

/// Rebuild all bloom filters from the current follower bitmaps.
/// Called after bulk load and periodically to correct for removals
/// (bloom filters don't support deletion).
pub fn build_all_bloom_filters(graph: &FollowGraph) {
    let mut rebuilt = 0u64;
    for entry in graph.followers.iter() {
        let uid = *entry.key();
        let followers_bm = entry.value();
        let count = followers_bm.len() as usize;

        if count == 0 {
            continue;
        }

        let mut bloom = new_bloom_filter(count);
        for follower_uid in followers_bm.iter() {
            bloom.set(&follower_uid);
        }
        graph.follower_blooms.insert(uid, bloom);
        rebuilt += 1;
    }
    tracing::info!("rebuilt bloom filters for {rebuilt} users");
}
