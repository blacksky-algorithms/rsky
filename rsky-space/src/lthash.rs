//! LtHash set commitment for permissioned repos.
//!
//! Per the permissioned-data proposal (§Commit digest): the repo digest is a
//! homomorphic set hash over the records a repo contains. It is order
//! independent, so two repos with the same records produce the same digest,
//! and add/remove are single cheap lane-wise operations.
//!
//! State is a fixed 2048-byte buffer read as 1024 little-endian `u16` lanes.
//! Each record maps to the UTF-8 bytes of `{collection}/{rkey}/{record_cid}`,
//! expanded to 2048 bytes with BLAKE3 in XOF mode, then folded lane-wise
//! (mod 2^16) into the state. The commit hash is `sha256(state)`.

use sha2::{Digest, Sha256};

const LANES: usize = 1024;
const STATE_BYTES: usize = 2048;

/// Expand an element to its 1024 little-endian `u16` lanes via BLAKE3 XOF.
fn element_lanes(element: &str) -> [u16; LANES] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(element.as_bytes());
    let mut out = [0u8; STATE_BYTES];
    hasher.finalize_xof().fill(&mut out);
    let mut lanes = [0u16; LANES];
    for (i, lane) in lanes.iter_mut().enumerate() {
        *lane = u16::from_le_bytes([out[i * 2], out[i * 2 + 1]]);
    }
    lanes
}

/// The canonical element string for a record: `{collection}/{rkey}/{cid}`.
pub fn element(collection: &str, rkey: &str, cid: &str) -> String {
    format!("{collection}/{rkey}/{cid}")
}

/// Incremental homomorphic set hash over a repo's records.
///
/// The empty repo's state is all zeroes. Adding then removing the same element
/// returns to the prior state, and the state is independent of operation order.
#[derive(Clone)]
pub struct LtHash {
    state: [u16; LANES],
}

impl Default for LtHash {
    fn default() -> Self {
        Self::new()
    }
}

impl LtHash {
    pub fn new() -> Self {
        Self {
            state: [0u16; LANES],
        }
    }

    /// Reconstruct an accumulator from a previously persisted 2048-byte state.
    pub fn from_state_bytes(bytes: &[u8; STATE_BYTES]) -> Self {
        let mut state = [0u16; LANES];
        for (i, lane) in state.iter_mut().enumerate() {
            *lane = u16::from_le_bytes([bytes[i * 2], bytes[i * 2 + 1]]);
        }
        Self { state }
    }

    /// Fold an element into the state (wrapping add, mod 2^16).
    pub fn add(&mut self, element: &str) {
        self.combine(element, false);
    }

    /// Remove an element from the state (wrapping subtract, mod 2^16).
    pub fn remove(&mut self, element: &str) {
        self.combine(element, true);
    }

    fn combine(&mut self, element: &str, subtract: bool) {
        let lanes = element_lanes(element);
        for (s, l) in self.state.iter_mut().zip(lanes.iter()) {
            *s = if subtract {
                s.wrapping_sub(*l)
            } else {
                s.wrapping_add(*l)
            };
        }
    }

    /// The 2048-byte little-endian state buffer (for persistence).
    pub fn state_bytes(&self) -> [u8; STATE_BYTES] {
        let mut buf = [0u8; STATE_BYTES];
        for (i, lane) in self.state.iter().enumerate() {
            let b = lane.to_le_bytes();
            buf[i * 2] = b[0];
            buf[i * 2 + 1] = b[1];
        }
        buf
    }

    /// The commit `hash`: `sha256(state)`, a 32-byte digest.
    pub fn hash(&self) -> [u8; 32] {
        Sha256::digest(self.state_bytes()).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_state_is_all_zeroes() {
        let lt = LtHash::new();
        assert_eq!(lt.state_bytes(), [0u8; STATE_BYTES]);
        // sha256 of 2048 zero bytes is deterministic; just assert stability.
        assert_eq!(lt.hash(), LtHash::new().hash());
    }

    #[test]
    fn add_then_remove_is_identity() {
        let e = element("app.bsky.feed.post", "3kabc", "bafyreiabc");
        let mut lt = LtHash::new();
        lt.add(&e);
        assert_ne!(lt.state_bytes(), [0u8; STATE_BYTES]);
        lt.remove(&e);
        assert_eq!(lt.state_bytes(), [0u8; STATE_BYTES]);
    }

    #[test]
    fn order_independent() {
        let a = element("c", "r1", "cid1");
        let b = element("c", "r2", "cid2");
        let c = element("c", "r3", "cid3");

        let mut x = LtHash::new();
        x.add(&a);
        x.add(&b);
        x.add(&c);

        let mut y = LtHash::new();
        y.add(&c);
        y.add(&a);
        y.add(&b);

        assert_eq!(x.hash(), y.hash());
        assert_eq!(x.state_bytes(), y.state_bytes());
    }

    #[test]
    fn roundtrip_state_bytes() {
        let mut lt = LtHash::new();
        lt.add(&element("c", "r", "cid"));
        let restored = LtHash::from_state_bytes(&lt.state_bytes());
        assert_eq!(restored.hash(), lt.hash());
    }

    #[test]
    fn distinct_sets_differ() {
        let mut x = LtHash::new();
        x.add(&element("c", "r1", "cid1"));
        let mut y = LtHash::new();
        y.add(&element("c", "r2", "cid2"));
        assert_ne!(x.hash(), y.hash());
    }
}
