//! Partial index of all repos which need a resync
//!
//! see `repo` for public-facing repo view of combined state.
//!
//! there is a resync index entry for a repo when
//! - repo status: `active`
//! - sync state: `desynchronized`
//!
//! if this was sql, we'd hold the `due_millis` as a field on the repo table.
//! here it only exists in the index.
//!
//! "rq|"||<host>||NUL||<due_millis: u64_be>||<did> => []
//!
//! the index is covering: the resync scheduler only need the DID to send a
//! `ScheduledResync` task to its actor.
//!
//! host partitioning is an optimization for the resync scheduler to round-robin
//! work across PDSes, to maximize backfill throughput.
//!
//! missing hosts are omitted (the NUL split will leave an empty string), which
//! shouldn't usually happen, but is possible and valid.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use metrics::counter;
use tracing::error;

use crate::metrics::{RESYNC_QUEUE_DEQUEUED_TOTAL, RESYNC_QUEUE_ENQUEUED_TOTAL};
use crate::resync_scheduler::QueuedResync;
use crate::{Did, Host, HostRegistry, StorageBatch, StorageEngine, StorageError};

use super::{
    AccountStatus, DecodeError, LoadError, PREFIX_REPO_INFO_IDX_RESYNC, RepoInfo, SyncStatus,
    slice_split_once,
};

fn encode_key(host: Option<&Host>, did: &Did, due: SystemTime) -> Vec<u8> {
    let host_bytes = host.map(|h| h.name().as_str().as_bytes()).unwrap_or(&[]);
    let did_bytes = did.as_str().as_bytes();
    // on-disk format is `u64` big-endian millis since unix epoch
    let due_at_ms = due
        .duration_since(UNIX_EPOCH)
        .expect("due after epoch")
        .as_millis() as u64;

    let mut k = Vec::with_capacity(
        PREFIX_REPO_INFO_IDX_RESYNC.len() + host_bytes.len() + 1 + 8 + did_bytes.len(),
    );
    k.extend_from_slice(PREFIX_REPO_INFO_IDX_RESYNC);
    k.extend_from_slice(host_bytes);
    k.push(0x00);
    k.extend_from_slice(&due_at_ms.to_be_bytes());
    k.extend_from_slice(did_bytes);
    k
}

fn decode_key(reg: &HostRegistry, k: &[u8]) -> Result<QueuedResync, DecodeError> {
    let (host_bytes, rest) = slice_split_once(k, &0x00).ok_or(DecodeError::MissingNullSeparator)?;

    let host = if host_bytes.is_empty() {
        None
    } else {
        let host_str =
            str::from_utf8(host_bytes).map_err(|err| DecodeError::NotUtf8 { what: "host", err })?;
        Some(reg.get(host_str)?)
    };

    decode_host_suffix(host, rest)
}

fn build_host_prefix(host: Option<&Host>) -> Vec<u8> {
    let host_bytes = host
        .map(|h| h.name().as_str().as_bytes())
        .unwrap_or_default();
    let mut p = Vec::with_capacity(PREFIX_REPO_INFO_IDX_RESYNC.len() + host_bytes.len() + 1);
    p.extend_from_slice(PREFIX_REPO_INFO_IDX_RESYNC);
    p.extend_from_slice(host_bytes);
    p.push(0x00);
    p
}

/// a suffix that is an (exclusive!) lower bound for a host prefix
fn build_host_next_suffix(did: &Did, due: SystemTime) -> Vec<u8> {
    let did_bytes = did.as_str().as_bytes();
    let due_at_ms = due
        .duration_since(UNIX_EPOCH)
        .expect("due after epoch")
        .as_millis() as u64;
    let mut s = Vec::with_capacity(8 + did_bytes.len() + 1);
    s.extend_from_slice(&due_at_ms.to_be_bytes());
    s.extend_from_slice(did_bytes);
    s.push(0x00); // next possible key is [key] + 0x00
    s
}

/// a key that is strictly after all items under this host
///
/// does not write the top-level prefix (it's for prefix-scans under it)
fn build_host_post_suffix(host: Option<&Host>) -> Vec<u8> {
    let host_bytes = host
        .map(|h| h.name().as_str().as_bytes())
        .unwrap_or(&[0x00]); // [0x00] is after []
    let mut p = Vec::with_capacity(host_bytes.len() + 1);
    p.extend_from_slice(host_bytes);
    p.push(0x01);
    p
}

fn decode_host_suffix(
    host: Option<Arc<Host>>,
    k_unprefixed: &[u8],
) -> Result<QueuedResync, DecodeError> {
    // the host prefix and nul-sep are already removed
    let Some((head, k_rest)) = k_unprefixed.split_first_chunk::<8>() else {
        return Err(DecodeError::InputTooShort);
    };
    let due = UNIX_EPOCH + Duration::from_millis(u64::from_be_bytes(*head));
    let did: Did = str::from_utf8(k_rest)
        .map_err(|err| DecodeError::NotUtf8 { what: "did", err })?
        .try_into()
        .map_err(DecodeError::BadDid)?;

    Ok(QueuedResync::new(host, did, due))
}

#[derive(Debug, Clone, PartialEq)]
enum Index {
    ShouldNot,
    Should(Partial),
}

#[derive(Debug, Clone, PartialEq)]
struct Partial {
    host: Arc<Host>,
    due: SystemTime,
}

impl Index {
    fn should(info: &RepoInfo) -> Self {
        match (&info.upstream_status, &info.sync_status) {
            (AccountStatus::Active, SyncStatus::Desynchronized(d)) => Self::Should(Partial {
                host: info.identity.pds_host.clone(),
                due: d.due_at(),
            }),
            _ => Self::ShouldNot,
        }
    }
}

impl QueuedResync {
    /// database-queue-ordered compare
    pub fn is_queued_after(&self, due: SystemTime, did: &Did) -> bool {
        if self.due() > due {
            return true;
        }
        // same-ms resyncs are ordered by Did bytes
        self.due() == due && self.did.as_str().as_bytes() > did.as_str().as_bytes()
    }

    fn from_partial(did: Did, Partial { host, due }: Partial) -> Self {
        let host = Some(host);
        Self::new(host, did, due)
    }

    pub(super) fn reconcile_for<E: StorageError, B: StorageBatch<E>>(
        did: &Did,
        prev: &RepoInfo,
        next: &RepoInfo,
        batch: &mut B,
    ) {
        let should_prev = Index::should(prev);
        let should_next = Index::should(next);

        // no change to index: short-circuit
        if should_prev == should_next {
            return;
        }

        // since all fields are in the key, *any* update requires deleting an
        // existing previous key
        if let Index::Should(p) = should_prev {
            Self::from_partial(did.clone(), p).delete(batch);
            counter!(RESYNC_QUEUE_DEQUEUED_TOTAL).increment(1);
        }

        // create or update in the index if we should next
        if let Index::Should(p) = should_next {
            Self::from_partial(did.clone(), p).store(batch);
            if let SyncStatus::Desynchronized(d) = &next.sync_status {
                counter!(RESYNC_QUEUE_ENQUEUED_TOTAL, "reason" => d.reason.name()).increment(1);
            } else {
                error!("METRICS BUG: QueuedResync expected to only index Desynchronized");
            }
        }
    }

    // TODO: make this more private (fix resync scheduler tests)
    pub(crate) fn store<E: StorageError, B: StorageBatch<E>>(&self, batch: &mut B) {
        let key = encode_key(self.host.as_deref(), &self.did, self.due());
        batch.put_queue(&key, &[]);
    }

    // TODO: make this more private (fix resync scheduler tests)
    pub(crate) fn delete<E: StorageError, B: StorageBatch<E>>(self, batch: &mut B) {
        let key = encode_key(self.host.as_deref(), &self.did, self.due());
        batch.delete_queue(&key);
    }

    /// Peek the host's earliest queued entry, regardless of due time.
    pub fn peek<S: StorageEngine>(
        storage: &S,
        host: Option<Arc<Host>>,
    ) -> Result<Option<Self>, LoadError<S::Error>> {
        let prefix = build_host_prefix(host.as_deref());
        let Some(item) = storage.scan_from_queue(&prefix, &[]).next() else {
            return Ok(None);
        };
        let (k, _v) = item.map_err(LoadError::Storage)?;
        let entry = decode_host_suffix(host, &k)?;
        Ok(Some(entry))
    }

    /// Peek the host's next queued resync strictly after this one
    pub fn peek_host_next<S: StorageEngine>(
        &self,
        storage: &S,
    ) -> Result<Option<Self>, LoadError<S::Error>> {
        let prefix = build_host_prefix(self.host.as_deref());
        let suffix = build_host_next_suffix(&self.did, self.due());
        let Some(item) = storage.scan_from_queue(&prefix, &suffix).next() else {
            return Ok(None);
        };
        let (k, _v) = item.map_err(LoadError::Storage)?;
        let entry = decode_host_suffix(self.host.clone(), &k)?;
        Ok(Some(entry))
    }

    /// Peek the host's next n queued resyncs strictly after this one
    pub fn peek_host_next_batch<S: StorageEngine>(
        &self,
        n: usize,
        storage: &S,
    ) -> Result<Vec<Self>, LoadError<S::Error>> {
        let prefix = build_host_prefix(self.host.as_deref());
        let suffix = build_host_next_suffix(&self.did, self.due());
        let mut out = Vec::with_capacity(n);
        for item in storage.scan_from_queue(&prefix, &suffix).take(n) {
            let (k, _v) = item.map_err(LoadError::Storage)?;
            let entry = decode_host_suffix(self.host.clone(), &k)?;
            out.push(entry);
        }
        Ok(out)
    }
}

/// Iterator to get one QueuedResync per host with items in its queue
///
/// items may be in the (far) future -- that's fine, they still get scheduling
pub struct NextQueuedByHost<'a, S: StorageEngine> {
    storage: &'a S,
    registry: &'a HostRegistry,
    next_scan_suffix: Vec<u8>,
    done: bool,
}

impl<'a, S: StorageEngine> NextQueuedByHost<'a, S> {
    pub fn new(storage: &'a S, registry: &'a HostRegistry) -> Self {
        Self {
            storage,
            registry,
            next_scan_suffix: vec![],
            done: false,
        }
    }
}

impl<'a, S: StorageEngine> NextQueuedByHost<'a, S> {
    fn next_inner(&mut self) -> Result<Option<QueuedResync>, LoadError<S::Error>> {
        let Some((k, _)) = self
            .storage
            .scan_from_queue(PREFIX_REPO_INFO_IDX_RESYNC, &self.next_scan_suffix)
            .next()
            .transpose()
            .map_err(LoadError::Storage)?
        else {
            return Ok(None);
        };
        let resync = decode_key(self.registry, &k)?;
        self.next_scan_suffix = build_host_post_suffix(resync.host.as_deref());
        Ok(Some(resync))
    }
}

impl<'a, S: StorageEngine> Iterator for NextQueuedByHost<'a, S> {
    type Item = Result<QueuedResync, LoadError<S::Error>>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        match self.next_inner() {
            Ok(Some(h)) => Some(Ok(h)),
            Ok(None) => {
                self.done = true;
                None
            }
            Err(e) => {
                self.done = true;
                Some(Err(e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{Host, Hostname};
    use crate::storage::engine::mem::MemEngine;

    fn h(s: &str) -> Arc<Host> {
        Arc::new(Host::raw(Hostname::new(s)))
    }
    fn s(s: &str) -> Option<Arc<Host>> {
        Some(h(s))
    }
    fn t(ms: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(ms)
    }

    /// Build a valid 32-char `did:plc:` from a short test label. Padded with
    /// 'a' to the required length; distinct labels produce distinct DIDs.
    fn plc_did(label: &str) -> Did {
        let mut s = label.to_string();
        while s.len() < 24 {
            s.push('a');
        }
        s.truncate(24);
        Did::new(format!("did:plc:{s}")).expect("valid plc did")
    }

    fn qr(host: Arc<Host>, did: Did, due: SystemTime) -> QueuedResync {
        QueuedResync::new(Some(host), did, due)
    }

    #[test]
    fn peek_empty_returns_none() {
        let eng = MemEngine::new();
        let got = QueuedResync::peek(&eng, s("example.com")).expect("peek ok");
        assert!(got.is_none());
    }

    #[test]
    fn enqueue_then_peek_roundtrips() {
        let eng = MemEngine::new();
        let mut b = eng.batch();
        qr(h("example.com"), plc_did("abc"), t(1_000)).store(&mut b);
        b.commit().expect("commit ok");

        let got = QueuedResync::peek(&eng, s("example.com"))
            .expect("peek ok")
            .expect("entry present");
        assert_eq!(got.host.clone().unwrap().name().as_str(), "example.com");
        assert_eq!(got.did, plc_did("abc"));
        assert_eq!(got.due(), t(1_000));
    }

    #[test]
    fn peek_returns_earliest_due() {
        let eng = MemEngine::new();
        let hc = h("h.com");
        let mut b = eng.batch();
        qr(Arc::clone(&hc), plc_did("a"), t(5_000)).store(&mut b);
        qr(Arc::clone(&hc), plc_did("b"), t(1_000)).store(&mut b);
        qr(Arc::clone(&hc), plc_did("c"), t(3_000)).store(&mut b);
        b.commit().unwrap();

        let got = QueuedResync::peek(&eng, Some(Arc::clone(&hc)))
            .unwrap()
            .unwrap();
        assert_eq!(got.did, plc_did("b"));
        assert_eq!(got.due(), t(1_000));
    }

    #[test]
    fn peek_is_host_scoped() {
        let eng = MemEngine::new();
        let ha = h("a.com");
        let hb = h("b.com");
        let mut b = eng.batch();
        qr(Arc::clone(&ha), plc_did("a"), t(1_000)).store(&mut b);
        qr(Arc::clone(&hb), plc_did("b"), t(500)).store(&mut b);
        b.commit().unwrap();

        let on_a = QueuedResync::peek(&eng, Some(Arc::clone(&ha)))
            .unwrap()
            .unwrap();
        assert_eq!(on_a.did, plc_did("a"));
        assert_eq!(on_a.due(), t(1_000));

        let on_b = QueuedResync::peek(&eng, Some(Arc::clone(&hb)))
            .unwrap()
            .unwrap();
        assert_eq!(on_b.did, plc_did("b"));
        assert_eq!(on_b.due(), t(500));
    }

    #[test]
    fn delete_removes_the_entry() {
        let eng = MemEngine::new();
        let hc = h("h.com");
        let mut b = eng.batch();
        qr(Arc::clone(&hc), plc_did("a"), t(1_000)).store(&mut b);
        qr(Arc::clone(&hc), plc_did("b"), t(2_000)).store(&mut b);
        b.commit().unwrap();

        let first = QueuedResync::peek(&eng, Some(Arc::clone(&hc)))
            .unwrap()
            .unwrap();
        assert_eq!(first.did, plc_did("a"));

        let mut b = eng.batch();
        first.delete(&mut b);
        b.commit().unwrap();

        let next = QueuedResync::peek(&eng, Some(Arc::clone(&hc)))
            .unwrap()
            .unwrap();
        assert_eq!(next.did, plc_did("b"));
        assert_eq!(next.due(), t(2_000));
    }

    fn registry() -> Arc<HostRegistry> {
        HostRegistry::new_default()
    }

    #[test]
    fn next_queued_by_host_empty_queue_yields_none() {
        let eng = MemEngine::new();
        let reg = registry();
        let mut iter = NextQueuedByHost::new(&eng, &reg);
        assert!(iter.next().is_none());
        // exhaustion is latched
        assert!(iter.next().is_none());
    }

    #[test]
    fn next_queued_by_host_single_host_returns_one_item() {
        let eng = MemEngine::new();
        let reg = registry();
        let mut b = eng.batch();
        qr(h("example.com"), plc_did("abc"), t(1_000)).store(&mut b);
        b.commit().unwrap();

        let mut iter = NextQueuedByHost::new(&eng, &reg);
        let entry = iter.next().expect("got one").expect("ok");
        assert_eq!(entry.host.clone().unwrap().name().as_str(), "example.com");
        assert_eq!(entry.did, plc_did("abc"));
        assert_eq!(entry.due(), t(1_000));

        assert!(iter.next().is_none());
    }

    #[test]
    fn next_queued_by_host_returns_earliest_per_host() {
        let eng = MemEngine::new();
        let reg = registry();
        let hh = h("h.com");
        let mut b = eng.batch();
        qr(Arc::clone(&hh), plc_did("a"), t(5_000)).store(&mut b);
        qr(Arc::clone(&hh), plc_did("b"), t(1_000)).store(&mut b);
        qr(Arc::clone(&hh), plc_did("c"), t(3_000)).store(&mut b);
        b.commit().unwrap();

        let mut iter = NextQueuedByHost::new(&eng, &reg);
        let entry = iter.next().expect("got one").expect("ok");
        assert_eq!(entry.host.clone().unwrap().name().as_str(), "h.com");
        // earliest-due of the three for h.com
        assert_eq!(entry.did, plc_did("b"));
        assert_eq!(entry.due(), t(1_000));

        // host has more durable entries but the iterator yields only the head
        assert!(iter.next().is_none());
    }

    #[test]
    fn next_queued_by_host_yields_one_per_distinct_host() {
        let eng = MemEngine::new();
        let reg = registry();
        let mut b = eng.batch();
        qr(h("a.com"), plc_did("a"), t(1_000)).store(&mut b);
        qr(h("b.com"), plc_did("b"), t(500)).store(&mut b);
        qr(h("c.com"), plc_did("c"), t(2_000)).store(&mut b);
        b.commit().unwrap();

        let items: Vec<_> = NextQueuedByHost::new(&eng, &reg)
            .collect::<Result<Vec<_>, _>>()
            .expect("all ok");
        assert_eq!(items.len(), 3);
        // order is by hostname bytewise (the key prefix after PREFIX_REPO_INFO_IDX_RESYNC)
        assert_eq!(items[0].host.as_ref().unwrap().name().as_str(), "a.com");
        assert_eq!(items[1].host.as_ref().unwrap().name().as_str(), "b.com");
        assert_eq!(items[2].host.as_ref().unwrap().name().as_str(), "c.com");
    }

    #[test]
    fn next_queued_by_host_yields_earliest_per_host_across_hosts() {
        let eng = MemEngine::new();
        let reg = registry();
        let mut b = eng.batch();
        // a.com: three entries, earliest at t=1_000
        qr(h("a.com"), plc_did("a1"), t(3_000)).store(&mut b);
        qr(h("a.com"), plc_did("a2"), t(1_000)).store(&mut b);
        qr(h("a.com"), plc_did("a3"), t(2_000)).store(&mut b);
        // b.com: two entries, earliest at t=500
        qr(h("b.com"), plc_did("b1"), t(500)).store(&mut b);
        qr(h("b.com"), plc_did("b2"), t(4_000)).store(&mut b);
        b.commit().unwrap();

        let mut iter = NextQueuedByHost::new(&eng, &reg);
        let a_head = iter.next().expect("a").expect("ok");
        assert_eq!(a_head.host.clone().unwrap().name().as_str(), "a.com");
        assert_eq!(a_head.did, plc_did("a2"));
        assert_eq!(a_head.due(), t(1_000));

        let b_head = iter.next().expect("b").expect("ok");
        assert_eq!(b_head.host.clone().unwrap().name().as_str(), "b.com");
        assert_eq!(b_head.did, plc_did("b1"));
        assert_eq!(b_head.due(), t(500));

        assert!(iter.next().is_none());
    }

    #[test]
    fn next_queued_by_host_yielded_host_is_registry_interned() {
        let eng = MemEngine::new();
        let reg = registry();
        let mut b = eng.batch();
        qr(h("example.com"), plc_did("abc"), t(1_000)).store(&mut b);
        b.commit().unwrap();

        let entry = NextQueuedByHost::new(&eng, &reg)
            .next()
            .expect("got one")
            .expect("ok");
        // a fresh registry.get for the same name returns the same Arc<Host>.
        let from_reg = reg.get("example.com").unwrap();
        assert!(Arc::ptr_eq(&entry.host.unwrap(), &from_reg));
    }

    #[test]
    fn next_queued_by_host_propagates_decode_error() {
        let eng = MemEngine::new();
        let reg = registry();
        // write a raw key with no NUL separator after the prefix. the engine
        // strips PREFIX_REPO_INFO_IDX_RESYNC during scan, so the iterator sees just
        // the bad bytes and decode_key fails at the host-separator step.
        let mut b = eng.batch();
        let mut bad_key = Vec::new();
        bad_key.extend_from_slice(PREFIX_REPO_INFO_IDX_RESYNC);
        bad_key.extend_from_slice(b"bytes_without_a_separator");
        b.put_queue(&bad_key, &[]);
        b.commit().unwrap();

        let mut iter = NextQueuedByHost::new(&eng, &reg);
        let err = iter.next().expect("got error").unwrap_err();
        assert!(matches!(
            err,
            LoadError::Decode(DecodeError::MissingNullSeparator)
        ));
        // exhaustion latched after an error
        assert!(iter.next().is_none());
    }
}
