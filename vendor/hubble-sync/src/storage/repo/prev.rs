//! sync11 state at its own key for smaller high-traffic writes
//!
//! see `repo` for public-facing repo view of combined state.
//!
//! "pi|"||<DID> =>
//!   <prev>||<rev>||<commits>||<others>||<delta>
//!
//! manual ser/de for mimimal size

use std::time::{Duration, SystemTime};

use super::{DecodeError, LoadError, PREFIX_REPO_PREV};
use crate::commit::OpKind;
use crate::storage::repo::FirehoseCommit;
use crate::{CommitObject, DaslCid, Did, StorageBatch, StorageEngine, StorageError, Tid};

const FUTURE_REV_TOLERANCE: Duration = Duration::from_secs(45);

#[derive(Debug)]
pub enum Sync11Outcome {
    /// ignoreable issues
    Drop(Sync11DropReason),
    /// requires fixing
    Desync(Sync11DesyncReason),
    /// transitions clean
    Pass,
}

#[derive(Debug)]
pub enum Sync11DropReason {
    /// rev not newer than the latest synchronized commit
    OldRev,
    /// `since` field required on non-first commit not found
    MissingSince,
}

#[derive(Debug)]
pub enum Sync11DesyncReason {
    /// sync1.1 fail: commit does not follow from current state
    DoesNotFollow,
    /// commit reject: not accepting future-Tid rev
    FutureRev,
}

#[derive(Debug, thiserror::Error)]
pub enum Sync11ResetError {
    #[error("rev not newer than the last synchronized commit")]
    OldRev,
    #[error("future-Tid rev in commit")]
    FutureRev,
}

#[derive(Debug, Clone)]
pub struct AccountSyncState {
    pub prev: DaslCid,
    pub rev: Tid,
    pub commits: u32,
    pub other_events: u32,
    pub records_count_delta: i32,
}

impl AccountSyncState {
    pub(super) fn key(did: &Did) -> Vec<u8> {
        let did = did.as_str().as_bytes();
        let mut k = Vec::with_capacity(PREFIX_REPO_PREV.len() + did.len());
        k.extend_from_slice(PREFIX_REPO_PREV);
        k.extend_from_slice(did);
        k
    }

    pub fn load<S: StorageEngine>(
        storage: &S,
        did: &Did,
    ) -> Result<Option<Self>, LoadError<S::Error>> {
        let Some(bytes) = storage.get(&Self::key(did)).map_err(LoadError::Storage)? else {
            return Ok(None);
        };
        let state = Self::try_from(bytes.as_slice())?;
        Ok(Some(state))
    }

    pub fn store<E: StorageError, B: StorageBatch<E>>(&self, did: &Did, batch: &mut B) {
        let bytes: Vec<u8> = self.into();
        batch.put(&Self::key(did), &bytes);
    }

    /// get a possibly-discontinuous commit state
    pub fn reset(
        existing: Option<&Self>,
        commit: &CommitObject,
        now: SystemTime,
    ) -> Result<Self, Sync11ResetError> {
        // if rev is not strictly greater than existing, drop this
        if let Some(ex) = existing
            && commit.rev <= ex.rev
        {
            return Err(Sync11ResetError::OldRev);
        }

        // from repo spec: drop future Tids for repo rev
        let (t, _clock_id) = commit.rev.into();
        if t.duration_since(now).unwrap_or(Duration::ZERO) > FUTURE_REV_TOLERANCE {
            return Err(Sync11ResetError::FutureRev);
        }

        Ok(Self {
            rev: commit.rev,
            prev: commit.data,
            commits: 0,
            records_count_delta: 0,
            other_events: 0,
        })
    }

    /// apply a commit that should follow from the current state
    pub fn apply<E: StorageError, B: StorageBatch<E>>(
        &mut self,
        did: &Did,
        commit: &FirehoseCommit,
        now: SystemTime,
        batch: &mut B,
    ) -> Sync11Outcome {
        // 5. if rev is not strictly greater, drop this commit
        if commit.rev <= self.rev {
            return Sync11Outcome::Drop(Sync11DropReason::OldRev);
        }

        // bonus checks:
        // - if we're here, this is not the initial commit: `since` required
        match commit.since {
            // - if `since` doesn't match our rev, something is probably wrong! but
            // the spec does not currently say we should reject! so just warn.
            None => {
                tracing::warn!(?did, rev = ?commit.rev, "ignoring with missing `since`");
                //return Sync11Outcome::Drop(Sync11DropReason::MissingSince);
            }
            Some(since) if since == self.rev => {
                tracing::trace!(?did, rev = ?commit.rev, "since matched prev rev, good.");
            }
            Some(since) => {
                tracing::warn!(
                    ?did,
                    rev = ?commit.rev,
                    prev = ?self.rev,
                    ?since,
                    "allowing commit with wrong `since` (should match prev)");
            }
        }

        // 6. check the commit's prevData against ours
        if commit.prev != self.prev {
            return Sync11Outcome::Desync(Sync11DesyncReason::DoesNotFollow);
        }

        // from repo spec: reject future Tids for repo rev
        let (t, _clock_id) = commit.rev.into();
        if t.duration_since(now).unwrap_or(Duration::ZERO) > FUTURE_REV_TOLERANCE {
            return Sync11Outcome::Desync(Sync11DesyncReason::FutureRev);
        }

        // well, we made it! update repo metrics and apply
        self.rev = commit.rev;
        self.prev = commit.data;
        self.commits = self.commits.saturating_add(1);
        self.records_count_delta += commit.ops.iter().fold(0, |acc: i32, op| match op.kind {
            OpKind::Create { .. } => acc.saturating_add(1),
            OpKind::Delete { .. } => acc.saturating_sub(1),
            _ => acc,
        });
        self.store(did, batch);
        Sync11Outcome::Pass
    }

    pub fn delete<E: StorageError, B: StorageBatch<E>>(did: &Did, batch: &mut B) {
        batch.delete(&Self::key(did));
    }

    pub fn scan<S: StorageEngine>(
        storage: &S,
        from_suffix: &[u8],
    ) -> impl Iterator<Item = Result<(Did, Self), LoadError<S::Error>>> {
        storage
            .scan_from(PREFIX_REPO_PREV, from_suffix)
            .filter_map(|pair| {
                let (suffix, bytes) = match pair {
                    Ok(pair) => pair,
                    Err(e) => return Some(Err(LoadError::Storage(e))),
                };
                // slot key is distinguishable by a NUL byte after the DID
                if suffix.contains(&0x00) {
                    return None; // filter slot out
                }
                let did = match std::str::from_utf8(&suffix) {
                    Ok(s) => Did::raw(s),
                    Err(err) => {
                        return Some(Err(DecodeError::NotUtf8 {
                            what: "DID in repo prev key",
                            err,
                        }
                        .into()));
                    }
                };
                match Self::try_from(bytes.as_slice()) {
                    Ok(state) => Some(Ok((did, state))),
                    Err(e) => Some(Err(e.into())),
                }
            })
    }
}

impl TryFrom<&[u8]> for AccountSyncState {
    type Error = DecodeError;
    fn try_from(bytes: &[u8]) -> Result<AccountSyncState, Self::Error> {
        // atproto sha256 cids are 4 + 32 bytes
        let (cid_bytes, rest) = bytes
            .split_first_chunk::<36>()
            .ok_or(DecodeError::InputTooShort)?;
        let prev = DaslCid::from_bytes_raw(cid_bytes)?;
        let (rev_bytes, rest) = rest
            .split_first_chunk::<8>()
            .ok_or(DecodeError::InputTooShort)?;
        let rev = u64::from_be_bytes(*rev_bytes).try_into()?;
        let (commits_bytes, rest) = rest
            .split_first_chunk::<4>()
            .ok_or(DecodeError::InputTooShort)?;
        let (other_events_bytes, rest) = rest
            .split_first_chunk::<4>()
            .ok_or(DecodeError::InputTooShort)?;
        let (records_count_delta_bytes, rest) = rest
            .split_first_chunk::<4>()
            .ok_or(DecodeError::InputTooShort)?;
        if !rest.is_empty() {
            return Err(DecodeError::ExtraBytes(rest.to_vec()));
        }
        Ok(Self {
            prev,
            rev,
            commits: u32::from_be_bytes(*commits_bytes),
            other_events: u32::from_be_bytes(*other_events_bytes),
            records_count_delta: i32::from_be_bytes(*records_count_delta_bytes),
        })
    }
}

impl From<&AccountSyncState> for Vec<u8> {
    fn from(acs: &AccountSyncState) -> Vec<u8> {
        let cid = acs.prev.as_bytes();
        let mut out = Vec::with_capacity(cid.len() + 8 + 4 + 4 + 4);
        out.extend_from_slice(cid);
        out.extend_from_slice(&acs.rev.to_raw_bytes());
        out.extend_from_slice(&acs.commits.to_be_bytes());
        out.extend_from_slice(&acs.other_events.to_be_bytes());
        out.extend_from_slice(&acs.records_count_delta.to_be_bytes());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::AccountSyncState;
    use super::*;
    use crate::StorageBatch;
    use crate::storage::engine::mem::MemEngine;

    /// build a 36-byte v1 dag-cbor sha256 CID stuffed with `seed` for the
    /// digest. handles dasl::cid's raw-bytes constructor for tests where
    /// we don't care about the actual content, just that it round-trips.
    fn test_cid(seed: u8) -> DaslCid {
        let mut bytes = [0u8; 36];
        bytes[0] = 0x01; // cidv1
        bytes[1] = 0x71; // dag-cbor codec
        bytes[2] = 0x12; // sha256 multihash
        bytes[3] = 0x20; // digest length = 32
        for b in &mut bytes[4..] {
            *b = seed;
        }
        DaslCid::from_bytes_raw(&bytes).expect("test cid")
    }
    fn sample_sync_state() -> AccountSyncState {
        AccountSyncState {
            prev: test_cid(0xAA),
            rev: 1234.try_into().unwrap(),
            commits: 10,
            other_events: 5,
            records_count_delta: 7,
        }
    }
    #[test]
    fn account_sync_state_load_returns_none_when_unstored() {
        let eng = MemEngine::new();
        let got = AccountSyncState::load(&eng, &Did::raw("did:plc:abc")).unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn account_sync_state_round_trip() {
        let eng = MemEngine::new();
        let state = sample_sync_state();
        let mut b = eng.batch();
        state.store(&Did::raw("did:plc:abc"), &mut b);
        b.commit().unwrap();

        let got = AccountSyncState::load(&eng, &Did::raw("did:plc:abc"))
            .unwrap()
            .expect("entry present");
        assert_eq!(got.prev.as_bytes(), state.prev.as_bytes());
        assert_eq!(got.rev, state.rev);
        assert_eq!(got.commits, state.commits);
        assert_eq!(got.other_events, state.other_events);
        assert_eq!(got.records_count_delta, state.records_count_delta);
    }

    #[test]
    fn account_sync_state_truncated_errors() {
        let eng = MemEngine::new();
        // write fewer than the 36 bytes needed for the prev CID.
        let mut b = eng.batch();
        b.put(&AccountSyncState::key(&Did::raw("did:plc:abc")), &[0u8; 10]);
        b.commit().unwrap();

        let err = AccountSyncState::load(&eng, &Did::raw("did:plc:abc")).unwrap_err();
        assert!(matches!(err, LoadError::Decode(DecodeError::InputTooShort)));
    }

    #[test]
    fn account_sync_state_round_trip_preserves_byte_layout() {
        // make sure the fixed fields round-trip even when they share byte
        // values that could mis-align if the decoder ever miscounted.
        let eng = MemEngine::new();
        let state = AccountSyncState {
            prev: test_cid(0xFF),
            // top bit must stay 0 to be a valid Tid
            rev: 0x2A_BB_CC_DD_11_22_33_44.try_into().unwrap(),
            commits: 0xDEAD_BEEF,
            other_events: 0xCAFE_BABE,
            records_count_delta: -42,
        };
        let mut b = eng.batch();
        state.store(&Did::raw("did:plc:xyz"), &mut b);
        b.commit().unwrap();
        let got = AccountSyncState::load(&eng, &Did::raw("did:plc:xyz"))
            .unwrap()
            .expect("present");
        assert_eq!(got.rev, state.rev);
        assert_eq!(got.commits, state.commits);
        assert_eq!(got.other_events, state.other_events);
        assert_eq!(got.records_count_delta, -42);
    }

    // --- scan over the `si|` index ---

    fn store_state(eng: &MemEngine, did: &Did, state: &AccountSyncState) {
        let mut b = eng.batch();
        state.store(did, &mut b);
        b.commit().unwrap();
    }

    fn scan_dids(eng: &MemEngine, from: &[u8]) -> Vec<String> {
        AccountSyncState::scan(eng, from)
            .map(|r| r.expect("scan item ok").0.as_str().to_owned())
            .collect()
    }

    #[test]
    fn scan_empty_yields_nothing() {
        let eng = MemEngine::new();
        assert!(scan_dids(&eng, &[]).is_empty());
    }

    #[test]
    fn scan_yields_states_in_did_order() {
        let eng = MemEngine::new();
        let state = sample_sync_state();
        // store out of order; scan must return bytewise-sorted.
        for label in ["did:plc:c", "did:plc:a", "did:plc:b"] {
            store_state(&eng, &Did::raw(label), &state);
        }
        assert_eq!(
            scan_dids(&eng, &[]),
            ["did:plc:a", "did:plc:b", "did:plc:c"]
        );
    }

    #[test]
    fn scan_filters_colocated_slot_key() {
        let eng = MemEngine::new();
        let did = Did::raw("did:plc:a");
        store_state(&eng, &did, &sample_sync_state());
        // the colocated commit-slot key (`si|<did>\0u`) shares the prefix.
        let mut slot_key = AccountSyncState::key(&did);
        slot_key.extend_from_slice(&[0x00, b'u']);
        let mut b = eng.batch();
        b.put(&slot_key, &[1, 2, 3]);
        b.commit().unwrap();

        assert_eq!(
            scan_dids(&eng, &[]),
            ["did:plc:a"],
            "slot key must be filtered"
        );
    }

    #[test]
    fn scan_from_resumes_strictly_after_cursor() {
        let eng = MemEngine::new();
        let state = sample_sync_state();
        for label in ["did:plc:a", "did:plc:b", "did:plc:c"] {
            store_state(&eng, &Did::raw(label), &state);
        }
        // resume after "a": the DID bytes + the next-key 0x00 (as `list_synced` builds it).
        let mut from = b"did:plc:a".to_vec();
        from.push(0x00);
        assert_eq!(scan_dids(&eng, &from), ["did:plc:b", "did:plc:c"]);
    }

    #[test]
    fn scan_propagates_non_utf8_did_error() {
        let eng = MemEngine::new();
        // a `si|` key with non-utf8 DID bytes (and no NUL, so not filtered as a
        // slot): scan must surface the decode error, not skip it.
        let mut key = PREFIX_REPO_PREV.to_vec();
        key.extend_from_slice(&[0xFF, 0xFF]);
        let mut b = eng.batch();
        b.put(&key, &[]);
        b.commit().unwrap();

        let err = AccountSyncState::scan(&eng, &[])
            .next()
            .expect("one item")
            .expect_err("non-utf8 DID must error");
        assert!(matches!(
            err,
            LoadError::Decode(DecodeError::NotUtf8 {
                what: "DID in repo prev key",
                ..
            })
        ));
    }
}
