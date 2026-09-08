//! Log of upstream status changes
//!
//! "rs|"||<DID>||NUL||<ts_reverse>||( <up_hostname>||<up_seq:i64_be> )?
//!   => UpstreamStatusDetail
//!
//! where ts_reverse is (u64::MAX - unix_millis), so that scanning starts from
//! most-recent (our current storage engine scan only goes one way ha)
//!
//! status changes that don't have an upstream can collide if they arrive in the
//! same millisecond. the last one will "win" and overwrite the other.
//!
//! status changes from different hosts that are processed in the same
//! millisecond can appear in the wrong order in the log -- the last-processed
//! will set the cached account status, but can be earlier in the log if its
//! hostname sorts lex-before the other upstream. a pretty far-edge case.
//!
//! hostname and seq are expected to either both be absent (if originating from
//! hubble-sync itself) or both present (if coming from an upstream). upstreams
//! should always send a sequence number, which is a host-unique sortable key.
//!
//! `up_seq` is the subscribeRepos sequence number, but sometimes we source an
//! upstream status change from an http request. for now, a seq=`-1` sentinel is
//! used for that case. gross. sorry.
//!
//! the initial state for a newly seen repo is `Active`, and does not appear in
//! the status change log.
//!
//! the log usually only records *status changes*, so a transition by two
//! different upstreams to the same new-status will only be recorded under the
//! first processed. (this is an actor-level, read-free filter rn).

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dasl::drisl;
use serde::{Deserialize, Serialize};

use super::{DecodeError, PREFIX_REPO_INFO_LOG_STATUS};
use crate::{
    AccountStatus, Did, Host, HostRegistry, LoadError, StorageBatch, StorageEngine, StorageError,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DbUpstreamStatusValue {
    pub status: AccountStatus,
}

fn encode_key(did: &Did, at: SystemTime, upstream: Option<&AccountStatusUpstream>) -> Vec<u8> {
    let did_bytes = did.as_str().as_bytes();
    let upstream_bytes = upstream
        .map(|AccountStatusUpstream { host, seq }| {
            let mut b = host.name().as_str().as_bytes().to_vec();
            b.extend_from_slice(&seq.to_be_bytes());
            b
        })
        .unwrap_or(vec![]);

    let key_len = PREFIX_REPO_INFO_LOG_STATUS.len()
        + did_bytes.len()
        + 1 // null sep
        + 8 // reverse timestamp
        + upstream_bytes.len();
    let mut k = Vec::with_capacity(key_len);

    let at_ms_desc = u64::MAX
        - at.duration_since(UNIX_EPOCH)
            .expect("post-epoch log time")
            .as_millis() as u64;

    k.extend_from_slice(PREFIX_REPO_INFO_LOG_STATUS);
    k.extend_from_slice(did_bytes);
    k.push(0x00);
    k.extend_from_slice(&at_ms_desc.to_be_bytes());
    k.extend_from_slice(&upstream_bytes);
    k
}

fn prefix(did: &Did) -> Vec<u8> {
    let did_bytes = did.as_str().as_bytes();
    let mut k = Vec::with_capacity(PREFIX_REPO_INFO_LOG_STATUS.len() + did_bytes.len() + 1);
    k.extend_from_slice(PREFIX_REPO_INFO_LOG_STATUS);
    k.extend_from_slice(did_bytes);
    k.push(0x00);
    k
}

/// decode an upstream status log key
fn decode_key_suffix(
    k_unprefixed: &[u8],
    hosts: &HostRegistry,
) -> Result<(SystemTime, Option<AccountStatusUpstream>), DecodeError> {
    let (at_desc_bytes, rest) = k_unprefixed
        .split_first_chunk::<8>()
        .ok_or(DecodeError::InputTooShort)?;
    let at_ms_desc = u64::MAX - u64::from_be_bytes(*at_desc_bytes);
    let at = UNIX_EPOCH + Duration::from_millis(at_ms_desc);

    let upstream = rest
        .split_last_chunk::<8>()
        .map(|(hostname_bytes, seq_bytes)| -> Result<_, DecodeError> {
            let hostname = str::from_utf8(hostname_bytes)
                .map_err(|err| DecodeError::NotUtf8 { what: "did", err })?;
            let host = hosts.get(hostname)?;
            let seq = i64::from_be_bytes(*seq_bytes);
            Ok(AccountStatusUpstream { host, seq })
        })
        .transpose()?;

    Ok((at, upstream))
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AccountStatusEvent {
    /// the target of the event
    #[serde(with = "crate::identity::did::did_atproto")]
    pub subject: Did,
    /// the time of the upstream account event
    pub at: SystemTime,
    /// the originating upstream host
    pub upstream: Option<AccountStatusUpstream>,
    /// the new account status
    pub status: AccountStatus,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AccountStatusUpstream {
    #[serde(serialize_with = "crate::host::serialize_name")]
    pub host: Arc<Host>,
    pub seq: i64,
}

impl AccountStatusEvent {
    fn key(&self) -> Vec<u8> {
        encode_key(&self.subject, self.at, self.upstream.as_ref())
    }

    fn val(&self) -> Vec<u8> {
        drisl::to_vec(&DbUpstreamStatusValue {
            status: self.status.clone(),
        })
        .unwrap()
    }

    pub fn insert<E: StorageError, B: StorageBatch<E>>(&self, batch: &mut B) {
        batch.put_queue(&self.key(), &self.val());
    }

    pub fn scan<S: StorageEngine>(
        did: &Did,
        from_suffix: &[u8],
        storage: &S,
        hosts: &HostRegistry,
    ) -> impl Iterator<Item = Result<Self, LoadError<S::Error>>> {
        storage.scan_from_queue(&prefix(did), from_suffix).map(|r| {
            let (k, v) = r.map_err(LoadError::Storage)?;
            let (at, upstream) = decode_key_suffix(&k, hosts)?;
            let DbUpstreamStatusValue { status } = drisl::from_slice(&v)?;
            Ok(Self {
                subject: did.clone(),
                at,
                upstream,
                status,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::engine::mem::MemEngine;

    fn ms(millis: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(millis)
    }

    fn subject() -> Did {
        Did::raw("did:plc:hdhoaan3xa3jiuq4fg4mefid")
    }

    fn hosts() -> Arc<HostRegistry> {
        HostRegistry::new_default()
    }

    /// insert all `events` (keyed by their own subject), then scan `did` back.
    fn roundtrip(
        did: &Did,
        events: Vec<AccountStatusEvent>,
        hosts: &HostRegistry,
    ) -> Vec<AccountStatusEvent> {
        let eng = MemEngine::new();
        let mut b = eng.batch();
        for e in &events {
            e.insert(&mut b);
        }
        b.commit().unwrap();
        AccountStatusEvent::scan(did, &[], &eng, hosts)
            .collect::<Result<Vec<_>, _>>()
            .expect("scan decodes")
    }

    #[test]
    fn roundtrips_with_upstream() {
        // host + seq live in the key; the host round-trips via the registry
        let hosts = hosts();
        let host = hosts.get("pds.example.com").expect("host interns");
        let event = AccountStatusEvent {
            subject: subject(),
            at: ms(1_700_000_000_500),
            upstream: Some(AccountStatusUpstream { host, seq: 42 }),
            status: AccountStatus::Deactivated,
        };
        assert_eq!(
            roundtrip(&subject(), vec![event.clone()], &hosts),
            vec![event]
        );
    }

    #[test]
    fn roundtrips_without_upstream() {
        // hubble-internal changes carry no upstream host/seq (empty key suffix)
        let hosts = hosts();
        let event = AccountStatusEvent {
            subject: subject(),
            at: ms(1_700_000_000_000),
            upstream: None,
            status: AccountStatus::Deleted,
        };
        assert_eq!(
            roundtrip(&subject(), vec![event.clone()], &hosts),
            vec![event]
        );
    }

    #[test]
    fn roundtrips_inactive_string_status() {
        // Inactive(String) puts a string in the cbor value
        let hosts = hosts();
        let event = AccountStatusEvent {
            subject: subject(),
            at: ms(1_700_000_001_000),
            upstream: None,
            status: AccountStatus::Inactive("weird-reason".to_string()),
        };
        assert_eq!(
            roundtrip(&subject(), vec![event.clone()], &hosts),
            vec![event]
        );
    }

    #[test]
    fn scans_most_recent_first() {
        // reverse-ts key => ascending scan yields newest first
        let hosts = hosts();
        let mk = |at| AccountStatusEvent {
            subject: subject(),
            at,
            upstream: None,
            status: AccountStatus::Active,
        };
        let got = roundtrip(
            &subject(),
            vec![mk(ms(100)), mk(ms(300)), mk(ms(200))],
            &hosts,
        );
        let ats: Vec<_> = got.iter().map(|e| e.at).collect();
        assert_eq!(ats, vec![ms(300), ms(200), ms(100)]);
    }

    #[test]
    fn scan_is_scoped_to_subject() {
        // the key is DID-prefixed, so a scan for another DID sees nothing
        let hosts = hosts();
        let event = AccountStatusEvent {
            subject: subject(),
            at: ms(1_700_000_000_000),
            upstream: None,
            status: AccountStatus::Deactivated,
        };
        let other = Did::raw("did:plc:aaaabbbbccccddddeeeeffff");
        assert!(roundtrip(&other, vec![event], &hosts).is_empty());
    }
}
