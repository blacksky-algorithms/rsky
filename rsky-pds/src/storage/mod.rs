use anyhow::Result;
use lexicon_cid::Cid;
use libipld::cbor::encode::write_null;
use libipld::cbor::DagCborCodec;
use libipld::codec::Encode;
use serde_cbor::Value as CborValue;
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::io::Write;
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

impl Encode<DagCborCodec> for Ipld {
    fn encode<W: Write>(&self, c: DagCborCodec, w: &mut W) -> Result<()> {
        match self {
            Self::Json(JsonValue::Null) => write_null(w),
            Self::Json(JsonValue::Bool(b)) => b.encode(c, w),
            Self::Json(JsonValue::Number(n)) => {
                if n.is_f64() {
                    n.as_f64().unwrap().encode(c, w)
                } else if n.is_u64() {
                    n.as_u64().unwrap().encode(c, w)
                } else {
                    n.as_i64().unwrap().encode(c, w)
                }
            }
            Self::Json(JsonValue::String(s)) => s.encode(c, w),
            Self::Json(JsonValue::Object(o)) => serde_json::to_vec(o)?.encode(c, w),
            Self::Json(JsonValue::Array(a)) => serde_json::to_vec(a)?.as_slice().encode(c, w),
            Self::Bytes(b) => b.as_slice().encode(c, w),
            Self::List(l) => l.encode(c, w),
            Self::Map(m) => m.encode(c, w),
            Self::Link(cid) => cid.encode(c, w),
            Self::String(s) => s.encode(c, w),
        }
    }
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
pub mod sql_repo;
pub mod sync_storage;
pub mod types;
