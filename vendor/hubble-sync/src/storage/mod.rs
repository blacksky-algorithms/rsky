pub mod crawl_state;
pub mod engine;
pub mod firehose_cursor;
pub mod host_info;
pub mod moderation_log;
pub mod repo;

use crate::host::HostnameError;
use crate::identity::DidError;
use crate::{CidParseError, Did, Slot, SlotDecodeError, StorageError};

pub use crawl_state::CrawlState;
pub use engine::StorageEngine;
pub use moderation_log::ModerateEvent;

/// key prefix, type-aliased to make it hard to make a wrong-length mistake
type P = &'static [u8; 3];

const PREFIX_CRAWL_STATE: P = b"cs|";

const PREFIX_FIREHOSE_CURSOR: P = b"fc|";

const PREFIX_HOST_INFO: P = b"hi|";

const PREFIX_MOD_LOG: P = b"ml|";

const PREFIX_REPO_INFO: P = b"ri|";
const PREFIX_REPO_INFO_IDX_RESYNC: P = b"rq|";
const PREFIX_REPO_INFO_IDX_COUNT: P = b"rc|";
const PREFIX_REPO_INFO_IDX_ACCOUNT_COUNT: P = b"ra|";
const PREFIX_REPO_INFO_LOG_STATUS: P = b"rs|";

const PREFIX_REPO_PENDING: P = b"pi|";
const PREFIX_REPO_PENDING_IDX_RETRY: P = b"pq|";

const PREFIX_REPO_PREV: P = b"si|";

/// any kind of persistent data loading problem
///
/// - i/o at the storage engine itself
/// - decoding of the bytes (corruption, hubble-sync bug, ..)
#[derive(Debug, thiserror::Error)]
pub enum LoadError<E: StorageError> {
    #[error("storage: {0}")]
    Storage(#[source] E),
    #[error("decode: {0}")]
    Decode(#[from] DecodeError),
    #[error("integrity: {0}")]
    Integrity(String),
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("dag-cbor: {0}")]
    DagCbor(#[from] dasl::drisl::DecodeError<std::convert::Infallible>),
    #[error("cid: {0}")]
    Cid(#[from] CidParseError),
    #[error("tid: {0}")]
    Tid(#[from] crate::tid::TidParseError),
    #[error("input too short")]
    InputTooShort,
    #[error("bad value bytes: {0:?}")]
    BadBytes(Vec<u8>),
    #[error("extra bytes found: {0:?}")]
    ExtraBytes(Vec<u8>),
    #[error("missing null-separator in key")]
    MissingNullSeparator,
    #[error("bytes not utf8 ({what}): {err}")]
    NotUtf8 {
        what: &'static str,
        #[source]
        err: std::str::Utf8Error,
    },
    #[error("hostname: {0}")]
    Hostname(#[from] HostnameError),
    #[error("bad DID: {0}")]
    BadDid(#[from] DidError),
    #[error("state slot: did={did}, slot={slot:?}, err={err:?}")]
    RepoSlotDecode {
        did: Did,
        slot: Slot,
        #[source]
        err: SlotDecodeError,
    },
}

// `?` convenience: wrap decode errors into load errors
impl<E: StorageError> From<dasl::drisl::DecodeError<std::convert::Infallible>> for LoadError<E> {
    fn from(e: dasl::drisl::DecodeError<std::convert::Infallible>) -> Self {
        Self::Decode(DecodeError::DagCbor(e))
    }
}
impl<E: StorageError> From<CidParseError> for LoadError<E> {
    fn from(e: CidParseError) -> Self {
        Self::Decode(DecodeError::Cid(e))
    }
}
impl<E: StorageError> From<HostnameError> for LoadError<E> {
    fn from(e: HostnameError) -> Self {
        Self::Decode(DecodeError::Hostname(e))
    }
}

/// str-like split_once for slices until `slice_split_once` lands
fn slice_split_once<'a, T: PartialEq>(who: &'a [T], what: &'a T) -> Option<(&'a [T], &'a [T])> {
    let found_index = who.iter().position(|x| x == what)?;
    Some((&who[..found_index], &who[found_index + 1..]))
}

/// system time serialization helper (we use u64 millis)
pub(crate) mod unix_ms_u64 {
    use serde::{Deserialize, Deserializer, Serializer, ser::Error};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    pub fn serialize<S: Serializer>(t: &SystemTime, ser: S) -> Result<S::Ok, S::Error> {
        let ms = t
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::custom("SystemTime before UNIX_EPOCH"))?
            .as_millis() as u64;
        ser.serialize_u64(ms)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<SystemTime, D::Error> {
        let ms = u64::deserialize(de)?;
        Ok(UNIX_EPOCH + Duration::from_millis(ms))
    }

    /// normalize a systemtime to ms (storage) resolution
    pub fn floor(t: SystemTime) -> SystemTime {
        let dt = t.duration_since(UNIX_EPOCH).expect("time after epoch");
        UNIX_EPOCH + Duration::from_millis(dt.as_millis() as u64)
    }
}
