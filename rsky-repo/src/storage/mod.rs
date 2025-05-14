use lexicon_cid::Cid;
use serde_cbor::Value as CborValue;
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use thiserror::Error;

/// Ipld
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Ipld {
    /// Represents a Cid.
    Link(Cid),
    /// Represents a list.
    List(Vec<Ipld>),
    /// Represents a map of strings to objects.
    Map(BTreeMap<String, Ipld>),
    /// String
    String(String),
    /// Represents a sequence of bytes.
    #[serde(with = "serde_bytes")]
    Bytes(Vec<u8>),
    /// Represents a Json Value
    Json(JsonValue),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ObjAndBytes {
    pub obj: CborValue,
    #[serde(with = "serde_bytes")]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CidAndRev {
    pub cid: Cid,
    pub rev: String,
}

#[derive(Error, Debug)]
pub enum RepoRootError {
    #[error("Repo root not found")]
    RepoRootNotFoundError,
}

pub mod memory_blockstore;
pub mod readable_blockstore;
pub mod sync_storage;
pub mod types;
