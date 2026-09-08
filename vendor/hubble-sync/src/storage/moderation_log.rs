//! Log of local moderation actions
//!
//! "ml|"||<ts_reverse>||<DID> => ModerationDetails
//!
//! where ts_reverse is (u64::MAX - unix_millis), so that scanning starts from
//! most-recent (our current storage engine scan only goes one way ha)
//!
//! two moderations can collide if they arrive during the same millisecond,
//! which will lose data but not result in wrong effective state -- since last-
//! arrived-wins the moderation change, the overwrite would be mostly harmless.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dasl::drisl;
use serde::{Deserialize, Serialize};

use super::{DecodeError, PREFIX_MOD_LOG, repo::ModAction};
use crate::{Did, LoadError, StorageBatch, StorageEngine, StorageError};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DbModValue<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<ModAction>, // None means clearing a previous mod action
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<&'a str>, // DID of the moderator or moderation service
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<&'a str>,
}

fn encode_key(at: SystemTime, did: &Did) -> Vec<u8> {
    let did_bytes = did.as_str().as_bytes();
    let at_ms = at
        .duration_since(UNIX_EPOCH)
        .expect("post-epoch log time")
        .as_millis() as u64;
    let at_rev = u64::MAX - at_ms;
    let mut k = Vec::with_capacity(PREFIX_MOD_LOG.len() + 8 + did_bytes.len());
    k.extend_from_slice(PREFIX_MOD_LOG);
    k.extend_from_slice(&at_rev.to_be_bytes());
    k.extend_from_slice(did_bytes);
    k
}

fn decode_key(k_unprefixed: &[u8]) -> Result<(SystemTime, Did), DecodeError> {
    let (at_rev_bytes, did_bytes) = k_unprefixed
        .split_first_chunk::<8>()
        .ok_or(DecodeError::InputTooShort)?;
    let at_ms = u64::MAX - u64::from_be_bytes(*at_rev_bytes);
    let at = UNIX_EPOCH + Duration::from_millis(at_ms);
    let did = str::from_utf8(did_bytes)
        .map_err(|err| DecodeError::NotUtf8 { what: "did", err })?
        .try_into()
        .map_err(DecodeError::BadDid)?;
    Ok((at, did))
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ModerateEvent {
    /// the time of the moderation event
    pub at: SystemTime,
    /// the account being moderated
    #[serde(with = "crate::identity::did::did_atproto")]
    pub subject: Did,
    /// the moderation, or None to clear it
    pub action: Option<ModAction>,
    /// optional entity doing the moderating (eg a moderator, or labeller, or..)
    #[serde(with = "crate::identity::did::did_atproto_opt")]
    pub source: Option<Did>,
    /// optional free-form contextual message
    pub message: Option<String>,
    /// optional reference (label triplet, link, ..) about the action
    pub reference: Option<String>,
}

impl ModerateEvent {
    fn key(&self) -> Vec<u8> {
        encode_key(self.at, &self.subject)
    }

    fn val(&self) -> Vec<u8> {
        drisl::to_vec(&DbModValue {
            action: self.action,
            source: self.source.as_ref().map(|s| s.as_str()),
            message: self.message.as_deref(),
            reference: self.reference.as_deref(),
        })
        .unwrap()
    }

    pub fn insert<E: StorageError, B: StorageBatch<E>>(&self, batch: &mut B) {
        batch.put_queue(&self.key(), &self.val());
    }

    pub fn scan<S: StorageEngine>(
        storage: &S,
        from_suffix: &[u8],
    ) -> impl Iterator<Item = Result<Self, LoadError<S::Error>>> {
        storage
            .scan_from_queue(PREFIX_MOD_LOG, from_suffix)
            .map(|r| {
                let (k, v) = r.map_err(LoadError::Storage)?;
                let (at, subject) = decode_key(&k)?;
                let DbModValue {
                    action,
                    source,
                    message,
                    reference,
                } = drisl::from_slice(&v)?;
                Ok(Self {
                    at,
                    subject,
                    action,
                    source: source.map(Did::raw),
                    message: message.map(str::to_string),
                    reference: reference.map(str::to_string),
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

    /// insert all `events`, then scan the whole log back (newest-first)
    fn roundtrip(events: Vec<ModerateEvent>) -> Vec<ModerateEvent> {
        let eng = MemEngine::new();
        let mut b = eng.batch();
        for e in &events {
            e.insert(&mut b);
        }
        b.commit().unwrap();
        ModerateEvent::scan(&eng, &[])
            .collect::<Result<Vec<_>, _>>()
            .expect("scan decodes")
    }

    #[test]
    fn roundtrips_full_action() {
        // `at` is built from whole ms, so key encoding round-trips exactly
        let event = ModerateEvent {
            at: ms(1_700_000_000_123),
            subject: subject(),
            action: Some(ModAction::Takedown),
            source: Some(Did::raw("did:plc:aaaabbbbccccddddeeeeffff")),
            message: Some("spam".to_string()),
            reference: Some("case:42".to_string()),
        };
        assert_eq!(roundtrip(vec![event.clone()]), vec![event]);
    }

    #[test]
    fn roundtrips_cleared_action() {
        // action = None clears a prior moderation; all context absent, so the
        // value is an (almost) empty cbor map -- exercises skip_serializing_if
        let event = ModerateEvent {
            at: ms(1_700_000_000_000),
            subject: subject(),
            action: None,
            source: None,
            message: None,
            reference: None,
        };
        assert_eq!(roundtrip(vec![event.clone()]), vec![event]);
    }

    #[test]
    fn scans_most_recent_first() {
        // reverse-ts key => ascending scan yields newest first
        let mk = |at| ModerateEvent {
            at,
            subject: subject(),
            action: Some(ModAction::Suspend),
            source: None,
            message: None,
            reference: None,
        };
        let got = roundtrip(vec![mk(ms(100)), mk(ms(300)), mk(ms(200))]);
        let ats: Vec<_> = got.iter().map(|e| e.at).collect();
        assert_eq!(ats, vec![ms(300), ms(200), ms(100)]);
    }
}
