use lexicon_cid::Cid;
use serde_cbor::Value as CborValue;
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use thiserror::Error;

/// Ipld
///
/// Links and bytes take the atproto data model's JSON forms (`{"$link": cid}`
/// and `{"$bytes": base64}`) and native CBOR otherwise. `Bytes` precedes `Map`
/// so a JSON `$bytes` object is read as bytes rather than as a map.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Ipld {
    /// Represents a Cid.
    #[serde(with = "link")]
    Link(Cid),
    /// Represents a list.
    List(Vec<Ipld>),
    /// Represents a sequence of bytes.
    #[serde(with = "rsky_lexicon::atproto_bytes")]
    Bytes(Vec<u8>),
    /// Represents a map of strings to objects.
    Map(BTreeMap<String, Ipld>),
    /// String
    String(String),
    /// Represents a Json Value
    Json(JsonValue),
}

mod link {
    use lexicon_cid::Cid;
    use serde::de::{Error, MapAccess, Visitor};
    use serde::ser::SerializeMap;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::fmt;

    pub fn serialize<S: Serializer>(cid: &Cid, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            let mut map = serializer.serialize_map(Some(1))?;
            map.serialize_entry("$link", &cid.to_string())?;
            map.end()
        } else {
            cid.serialize(serializer)
        }
    }

    /// Accepts the JSON `{"$link": cid}` object and a DAG-CBOR tag-42 link,
    /// read from the value itself for the same reason as `atproto_bytes`.
    /// An untagged byte string is never a link, even when its bytes happen
    /// to parse as a CID.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Cid, D::Error> {
        deserializer.deserialize_any(LinkVisitor)
    }

    struct LinkVisitor;

    impl<'de> Visitor<'de> for LinkVisitor {
        type Value = Cid;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a CID link or a {\"$link\": cid} object")
        }

        fn visit_newtype_struct<D: Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> Result<Cid, D::Error> {
            let bytes = serde_bytes::ByteBuf::deserialize(deserializer)?;
            Cid::try_from(bytes.as_ref()).map_err(Error::custom)
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Cid, A::Error> {
            let Some((key, link)) = map.next_entry::<String, String>()? else {
                return Err(Error::custom("expected a \"$link\" key"));
            };
            if key != "$link" || map.next_key::<String>()?.is_some() {
                return Err(Error::custom("expected a single \"$link\" key"));
            }
            link.parse().map_err(Error::custom)
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
pub mod sync_storage;
pub mod types;
