//! Relay parity comparator.
//!
//! Subscribes to several relays, keys every `#commit` and `#sync` by (repo, rev,
//! commit CID) plus a hash of its normalized ops, and reports per window what each
//! relay missed, duplicated or altered relative to a reference, with matched-commit
//! latency. Identity and account events carry no unique identity, so they are
//! compared as per-DID end state.
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::validator::event::{SubscribeReposCommitOperation, SubscribeReposEvent};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CommitIdentity {
    pub did: String,
    pub rev: String,
    pub cid: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    pub identity: CommitIdentity,
    pub payload: [u8; 32],
    pub seen_at: Instant,
}

/// Per-DID state derived from identity/account events, compared at window end.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DidState {
    pub handle: Option<String>,
    pub active: Option<bool>,
    pub status: Option<String>,
}

fn op_digest(ops: &[SubscribeReposCommitOperation]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    let mut sorted: Vec<&SubscribeReposCommitOperation> = ops.iter().collect();
    sorted.sort();
    for op in sorted {
        match op {
            SubscribeReposCommitOperation::Create { path, cid } => {
                hasher.update(b"create|");
                hasher.update(path.as_bytes());
                hasher.update(cid.to_bytes());
            }
            SubscribeReposCommitOperation::Update { path, cid, prev } => {
                hasher.update(b"update|");
                hasher.update(path.as_bytes());
                hasher.update(cid.to_bytes());
                if let Some(prev) = prev {
                    hasher.update(prev.to_bytes());
                }
            }
            SubscribeReposCommitOperation::Delete { path, cid, prev } => {
                hasher.update(b"delete|");
                hasher.update(path.as_bytes());
                if let Some(cid) = cid {
                    hasher.update(cid.to_bytes());
                }
                if let Some(prev) = prev {
                    hasher.update(prev.to_bytes());
                }
            }
        }
        hasher.update(b"\n");
    }
    hasher.finalize().into()
}

/// What one relay contributed to a window.
#[derive(Debug, Default)]
pub struct RelayWindow {
    pub commits: BTreeMap<CommitIdentity, Vec<Observation>>,
    pub states: BTreeMap<String, DidState>,
    pub frames: u64,
}

impl RelayWindow {
    pub fn observe(&mut self, frame: &[u8], seen_at: Instant) {
        self.frames += 1;
        let Ok(Some(event)) = SubscribeReposEvent::parse(frame) else {
            return;
        };
        match &event {
            SubscribeReposEvent::Commit(commit) => {
                let identity = CommitIdentity {
                    did: commit.did.clone(),
                    rev: commit.rev.to_string(),
                    cid: commit.commit.to_string(),
                };
                let payload = op_digest(&commit.ops);
                self.commits.entry(identity.clone()).or_default().push(Observation {
                    identity,
                    payload,
                    seen_at,
                });
                self.states.entry(commit.did.clone()).or_default().active.get_or_insert(true);
            }
            SubscribeReposEvent::Sync(sync) => {
                if let Ok(Some((_, head))) = event.commit() {
                    let identity = CommitIdentity {
                        did: sync.did.clone(),
                        rev: sync.rev.to_string(),
                        cid: head.to_string(),
                    };
                    self.commits.entry(identity.clone()).or_default().push(Observation {
                        identity,
                        payload: [0; 32],
                        seen_at,
                    });
                }
            }
            SubscribeReposEvent::Identity(identity) => {
                let state = self.states.entry(identity.did.clone()).or_default();
                if identity.handle.is_some() {
                    state.handle.clone_from(&identity.handle);
                }
            }
            SubscribeReposEvent::Account(account) => {
                let state = self.states.entry(account.did.clone()).or_default();
                state.active = Some(account.active);
                state.status = account.status.as_ref().map(ToString::to_string);
            }
            SubscribeReposEvent::Labels(_) => {}
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RelayReport {
    pub relay: String,
    pub frames: u64,
    pub commits: usize,
    pub missing: usize,
    pub duplicates: usize,
    pub payload_mismatches: usize,
    pub extra: usize,
    pub latency_p50_ms: Option<u64>,
    pub latency_p99_ms: Option<u64>,
    pub converged_dids: usize,
    pub diverged_dids: usize,
}

fn percentile(sorted: &[u64], p: f64) -> Option<u64> {
    if sorted.is_empty() {
        return None;
    }
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted.get(idx).copied()
}

/// Compares `candidate` against `reference` for one window.
#[must_use]
pub fn compare(relay: &str, reference: &RelayWindow, candidate: &RelayWindow) -> RelayReport {
    let mut missing = 0;
    let mut duplicates = 0;
    let mut payload_mismatches = 0;
    let mut latencies = Vec::new();
    for (identity, ref_obs) in &reference.commits {
        match candidate.commits.get(identity) {
            None => missing += 1,
            Some(obs) => {
                if obs.len() > 1 {
                    duplicates += obs.len() - 1;
                }
                let ref_first = &ref_obs[0];
                let cand_first = &obs[0];
                if ref_first.payload != cand_first.payload {
                    payload_mismatches += 1;
                }
                let delta = cand_first.seen_at.saturating_duration_since(ref_first.seen_at);
                #[expect(clippy::cast_possible_truncation)]
                latencies.push(delta.as_millis() as u64);
            }
        }
    }
    let extra = candidate.commits.keys().filter(|k| !reference.commits.contains_key(*k)).count();
    latencies.sort_unstable();
    let mut converged = 0;
    let mut diverged = 0;
    for (did, ref_state) in &reference.states {
        match candidate.states.get(did) {
            Some(state) if state == ref_state => converged += 1,
            _ => diverged += 1,
        }
    }
    RelayReport {
        relay: relay.to_owned(),
        frames: candidate.frames,
        commits: candidate.commits.len(),
        missing,
        duplicates,
        payload_mismatches,
        extra,
        latency_p50_ms: percentile(&latencies, 0.5),
        latency_p99_ms: percentile(&latencies, 0.99),
        converged_dids: converged,
        diverged_dids: diverged,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Thresholds {
    pub max_missing_ratio: f64,
    pub max_latency_p50: Duration,
}

impl RelayReport {
    /// True when the relay is within the thresholds against a reference of
    /// `reference_commits` commits.
    #[must_use]
    pub fn passes(&self, reference_commits: usize, thresholds: Thresholds) -> bool {
        #[expect(clippy::cast_precision_loss)]
        let missing_ratio = if reference_commits == 0 {
            0.0
        } else {
            self.missing as f64 / reference_commits as f64
        };
        missing_ratio <= thresholds.max_missing_ratio
            && self.duplicates == 0
            && self.payload_mismatches == 0
            && self.latency_p50_ms.is_none_or(|p50| {
                #[expect(clippy::cast_possible_truncation)]
                let limit = thresholds.max_latency_p50.as_millis() as u64;
                p50 <= limit
            })
    }
}

/// Collects frames from every relay for one window and produces reports.
#[derive(Debug, Default)]
pub struct Session {
    pub windows: BTreeMap<String, RelayWindow>,
}

impl Session {
    pub fn observe(&mut self, relay: &str, frame: &[u8], seen_at: Instant) {
        self.windows.entry(relay.to_owned()).or_default().observe(frame, seen_at);
    }

    #[must_use]
    pub fn reports(&self, reference: &str) -> Vec<RelayReport> {
        let Some(reference_window) = self.windows.get(reference) else {
            return Vec::new();
        };
        self.windows
            .iter()
            .filter(|(name, _)| name.as_str() != reference)
            .map(|(name, window)| compare(name, reference_window, window))
            .collect()
    }

    #[must_use]
    pub fn reference_commits(&self, reference: &str) -> usize {
        self.windows.get(reference).map_or(0, |w| w.commits.len())
    }

    #[must_use]
    pub fn relays(&self) -> BTreeSet<String> {
        self.windows.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Cursor;
    use crate::validator::event::{SubscribeReposAccount, SubscribeReposIdentity};

    #[derive(serde::Deserialize)]
    struct FrameFixture {
        kind: String,
        frame_base64: String,
    }

    fn decode_base64(input: &str) -> Vec<u8> {
        let table: Vec<u8> =
            (b'A'..=b'Z').chain(b'a'..=b'z').chain(b'0'..=b'9').chain([b'+', b'/']).collect();
        let mut bits = 0u32;
        let mut nbits = 0;
        let mut out = Vec::new();
        for c in input.bytes().filter(|c| *c != b'=') {
            let v = u32::try_from(table.iter().position(|t| *t == c).unwrap()).unwrap();
            bits = (bits << 6) | v;
            nbits += 6;
            if nbits >= 8 {
                nbits -= 8;
                out.push(((bits >> nbits) & 0xff) as u8);
            }
        }
        out
    }

    fn fixture(kind: &str) -> Vec<u8> {
        let fixtures: Vec<FrameFixture> =
            serde_json::from_str(include_str!("../tests/interop/subscribe-repos-frames.json"))
                .unwrap();
        decode_base64(&fixtures.into_iter().find(|f| f.kind == kind).unwrap().frame_base64)
    }

    fn frame(event: SubscribeReposEvent, seq: u64) -> Vec<u8> {
        event.serialize(0, Cursor::from(seq)).unwrap()
    }

    fn altered_ops(kind: &str) -> Vec<u8> {
        let raw = fixture(kind);
        let SubscribeReposEvent::Commit(mut commit) =
            SubscribeReposEvent::parse(&raw).unwrap().unwrap()
        else {
            panic!("commit")
        };
        for op in &mut commit.ops {
            let (SubscribeReposCommitOperation::Create { path, .. }
            | SubscribeReposCommitOperation::Update { path, .. }
            | SubscribeReposCommitOperation::Delete { path, .. }) = op;
            path.push_str(".altered");
        }
        frame(SubscribeReposEvent::Commit(commit), 1)
    }

    #[test]
    fn identical_streams_report_nothing_missing() {
        let mut session = Session::default();
        let t0 = Instant::now();
        for (i, kind) in
            ["create", "update", "delete", "sync", "identity", "account"].iter().enumerate()
        {
            let raw = fixture(kind);
            session.observe("ref", &raw, t0 + Duration::from_millis(i as u64));
            session.observe("cand", &raw, t0 + Duration::from_millis(i as u64 + 5));
        }
        let reports = session.reports("ref");
        assert_eq!(reports.len(), 1);
        let r = &reports[0];
        assert_eq!(r.relay, "cand");
        assert_eq!(r.commits, 4);
        assert_eq!((r.missing, r.duplicates, r.payload_mismatches, r.extra), (0, 0, 0, 0));
        assert_eq!(r.latency_p50_ms, Some(5));
        assert_eq!(r.latency_p99_ms, Some(5));
        assert_eq!(r.diverged_dids, 0);
        assert!(r.converged_dids >= 4);
        assert!(r.passes(
            session.reference_commits("ref"),
            Thresholds { max_missing_ratio: 0.005, max_latency_p50: Duration::from_secs(2) }
        ));
        assert_eq!(session.relays().len(), 2);
        assert!(session.reports("nope").is_empty());
    }

    #[test]
    fn missing_duplicate_altered_and_extra_are_counted_separately() {
        let mut session = Session::default();
        let t0 = Instant::now();
        session.observe("ref", &fixture("create"), t0);
        session.observe("ref", &fixture("update"), t0);
        session.observe("ref", &fixture("delete"), t0);
        // candidate: create twice (duplicate), update altered (mismatch), delete missing, sync extra
        session.observe("cand", &fixture("create"), t0);
        session.observe("cand", &fixture("create"), t0);
        session.observe("cand", &altered_ops("update"), t0);
        session.observe("cand", &fixture("sync"), t0);
        session.observe("cand", b"garbage", t0);
        let r = &session.reports("ref")[0];
        assert_eq!(r.frames, 5);
        assert_eq!(r.missing, 1);
        assert_eq!(r.duplicates, 1);
        assert_eq!(r.payload_mismatches, 1);
        assert_eq!(r.extra, 1);
        assert!(!r.passes(
            3,
            Thresholds { max_missing_ratio: 0.0, max_latency_p50: Duration::from_secs(1) }
        ));
        let mut clean = r.clone();
        clean.missing = 0;
        clean.duplicates = 0;
        clean.payload_mismatches = 0;
        assert!(clean.passes(
            0,
            Thresholds { max_missing_ratio: 0.0, max_latency_p50: Duration::from_secs(1) }
        ));
        clean.latency_p50_ms = Some(5000);
        assert!(!clean.passes(
            3,
            Thresholds { max_missing_ratio: 0.5, max_latency_p50: Duration::from_secs(1) }
        ));
    }

    #[test]
    fn repeated_identity_notifications_are_not_duplicates() {
        let mut session = Session::default();
        let t0 = Instant::now();
        let make = |seq: u64, handle: Option<&str>| {
            frame(
                SubscribeReposEvent::Identity(SubscribeReposIdentity {
                    seq,
                    did: "did:plc:a".to_owned(),
                    time: "2026-01-01T00:00:00.000Z".to_owned(),
                    time_dt: chrono::DateTime::UNIX_EPOCH,
                    handle: handle.map(ToOwned::to_owned),
                }),
                seq,
            )
        };
        session.observe("ref", &make(1, Some("a.test")), t0);
        session.observe("cand", &make(1, Some("a.test")), t0);
        session.observe("cand", &make(2, Some("a.test")), t0);
        session.observe("cand", &make(3, None), t0);
        let account = frame(
            SubscribeReposEvent::Account(SubscribeReposAccount {
                seq: 4,
                did: "did:plc:a".to_owned(),
                time: "2026-01-01T00:00:00.000Z".to_owned(),
                time_dt: chrono::DateTime::UNIX_EPOCH,
                active: false,
                status: Some(crate::validator::event::AccountStatus::Deactivated),
            }),
            4,
        );
        session.observe("cand", &account, t0);
        let r = &session.reports("ref")[0];
        assert_eq!(r.duplicates, 0);
        assert_eq!(r.converged_dids, 0);
        assert_eq!(r.diverged_dids, 1, "candidate deactivated where reference did not");
        session.observe("ref", &account, t0);
        let r = &session.reports("ref")[0];
        assert_eq!(r.converged_dids, 1);
    }

    #[test]
    fn op_digest_is_order_independent_and_covers_all_variants() {
        let cid = cid::Cid::try_from("bafyreigbtj4x7ip5legnfznufuopl4sg4knzc2cof6duas4b3q2fy6swua")
            .unwrap();
        let ops = || {
            vec![
                SubscribeReposCommitOperation::Create { path: "a".to_owned(), cid },
                SubscribeReposCommitOperation::Update {
                    path: "b".to_owned(),
                    cid,
                    prev: Some(cid),
                },
                SubscribeReposCommitOperation::Delete {
                    path: "c".to_owned(),
                    cid: Some(cid),
                    prev: Some(cid),
                },
                SubscribeReposCommitOperation::Delete {
                    path: "d".to_owned(),
                    cid: None,
                    prev: None,
                },
                SubscribeReposCommitOperation::Update { path: "e".to_owned(), cid, prev: None },
            ]
        };
        let forward = op_digest(&ops());
        let mut reversed = ops();
        reversed.reverse();
        assert_eq!(forward, op_digest(&reversed));
        assert_eq!(percentile(&[], 0.5), None);
        assert_eq!(percentile(&[1, 2, 3, 4], 0.5), Some(3));
    }
}
