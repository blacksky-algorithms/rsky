//! Permissioned-record bytes: the atproto data model over DAG-CBOR.
//!
//! A repo host stores permissioned records shape-agnostically — it checks that
//! the value is a well-formed atproto data-model object under a size cap and
//! nothing more. Lexicon validation of permissioned records happens at the
//! consuming boundary, not here, because records in a space legitimately carry
//! space URIs in fields a public lexicon declares as `at-uri`.
//!
//! JSON ↔ DAG-CBOR follows the atproto data model: `{"$link": "…"}` is a CID
//! link, `{"$bytes": "…"}` is base64 bytes, and floats are rejected.

use base64::Engine;
use ipld_core::ipld::Ipld;
use lexicon_cid::multihash::Multihash;
use lexicon_cid::Cid;
use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};

use crate::error::{Result, SpaceError};

const SHA2_256: u64 = 0x12;
/// DAG-CBOR multicodec, the codec of every permissioned record block.
pub const DAG_CBOR: u64 = 0x71;

/// The CID of an already-encoded DAG-CBOR block.
pub fn dag_cbor_cid(bytes: &[u8]) -> Cid {
    let digest = Sha256::digest(bytes);
    let multihash = Multihash::wrap(SHA2_256, &digest).expect("sha256 digest fits in multihash");
    Cid::new_v1(DAG_CBOR, multihash)
}

fn b64() -> base64::engine::general_purpose::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn decode_err(msg: impl Into<String>) -> SpaceError {
    SpaceError::Decode(msg.into())
}

fn json_to_ipld(value: &Value) -> Result<Ipld> {
    Ok(match value {
        Value::Null => Ipld::Null,
        Value::Bool(b) => Ipld::Bool(*b),
        Value::String(s) => Ipld::String(s.clone()),
        Value::Number(n) => Ipld::Integer(
            n.as_i64()
                .map(i128::from)
                .ok_or_else(|| decode_err("only integer numbers are representable"))?,
        ),
        Value::Array(items) => Ipld::List(items.iter().map(json_to_ipld).collect::<Result<_>>()?),
        Value::Object(map) => match tagged(map) {
            Some(("$link", Value::String(s))) => Ipld::Link(
                s.parse::<Cid>()
                    .map_err(|e| decode_err(format!("invalid $link: {e}")))?,
            ),
            Some(("$bytes", Value::String(s))) => Ipld::Bytes(
                b64()
                    .decode(s)
                    .map_err(|e| decode_err(format!("invalid $bytes: {e}")))?,
            ),
            _ => Ipld::Map(
                map.iter()
                    .map(|(k, v)| Ok((k.clone(), json_to_ipld(v)?)))
                    .collect::<Result<_>>()?,
            ),
        },
    })
}

/// A single-entry map carrying one of the data model's reserved `$` keys.
fn tagged(map: &Map<String, Value>) -> Option<(&str, &Value)> {
    if map.len() != 1 {
        return None;
    }
    let (key, value) = map.iter().next()?;
    matches!(key.as_str(), "$link" | "$bytes").then_some((key.as_str(), value))
}

fn ipld_to_json(value: &Ipld) -> Result<Value> {
    Ok(match value {
        Ipld::Null => Value::Null,
        Ipld::Bool(b) => Value::Bool(*b),
        Ipld::String(s) => Value::String(s.clone()),
        Ipld::Integer(i) => Value::Number(Number::from(
            i64::try_from(*i).map_err(|_| decode_err("integer out of range"))?,
        )),
        Ipld::Float(_) => return Err(decode_err("floats are not part of the atproto data model")),
        Ipld::Bytes(b) => serde_json::json!({ "$bytes": b64().encode(b) }),
        Ipld::Link(cid) => serde_json::json!({ "$link": cid.to_string() }),
        Ipld::List(items) => Value::Array(items.iter().map(ipld_to_json).collect::<Result<_>>()?),
        Ipld::Map(map) => Value::Object(
            map.iter()
                .map(|(k, v)| Ok((k.clone(), ipld_to_json(v)?)))
                .collect::<Result<_>>()?,
        ),
    })
}

/// Encode a record value to canonical DAG-CBOR. The value must be an object;
/// no other structural or lexicon constraint is applied.
pub fn encode_record(value: &Value, max_bytes: usize) -> Result<Vec<u8>> {
    if !value.is_object() {
        return Err(decode_err("record must be an object"));
    }
    let ipld = json_to_ipld(value)?;
    let bytes = serde_ipld_dagcbor::to_vec(&ipld).map_err(|e| decode_err(e.to_string()))?;
    if bytes.len() > max_bytes {
        return Err(SpaceError::RecordTooLarge {
            size: bytes.len(),
            max: max_bytes,
        });
    }
    Ok(bytes)
}

/// Decode stored DAG-CBOR record bytes back to their JSON representation.
pub fn decode_record(bytes: &[u8]) -> Result<Value> {
    let ipld: Ipld =
        serde_ipld_dagcbor::from_slice(bytes).map_err(|e| decode_err(e.to_string()))?;
    ipld_to_json(&ipld)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const MAX: usize = 64 * 1024;

    #[test]
    fn roundtrips_the_data_model() {
        let value = json!({
            "$type": "app.bsky.feed.post",
            "text": "hi",
            "count": 3,
            "flag": true,
            "nothing": null,
            "tags": ["a", "b"],
            "embed": {
                "image": {"$link": "bafkreiabc2hs4mmvxbtmvj47xrslw6ijbcbtoz2jyxbjyqmjkgu3f4z2wq"},
                "raw": {"$bytes": "AQID"}
            }
        });
        let bytes = encode_record(&value, MAX).unwrap();
        assert_eq!(decode_record(&bytes).unwrap(), value);
    }

    #[test]
    fn encoding_is_canonical_and_order_independent() {
        let a = encode_record(&json!({"a": 1, "b": 2}), MAX).unwrap();
        let b = encode_record(&json!({"b": 2, "a": 1}), MAX).unwrap();
        assert_eq!(a, b);
        assert_eq!(dag_cbor_cid(&a), dag_cbor_cid(&b));
    }

    #[test]
    fn cid_is_a_dag_cbor_sha256_link() {
        let cid = dag_cbor_cid(&encode_record(&json!({"a": 1}), MAX).unwrap());
        assert_eq!(cid.codec(), DAG_CBOR);
        assert_eq!(cid.hash().code(), SHA2_256);
        assert!(cid.to_string().starts_with("bafyrei"));
    }

    #[test]
    fn a_space_uri_is_stored_verbatim() {
        let uri =
            "at://did:plc:auth/space/community.blacksky.feed/main/did:plc:a/app.bsky.feed.post/3k";
        let value = json!({"subject": {"uri": uri, "cid": "bafyreia"}});
        let bytes = encode_record(&value, MAX).unwrap();
        assert_eq!(decode_record(&bytes).unwrap(), value);
    }

    #[test]
    fn rejects_non_objects_floats_and_oversize() {
        assert!(matches!(
            encode_record(&json!([1, 2]), MAX),
            Err(SpaceError::Decode(_))
        ));
        assert!(matches!(
            encode_record(&json!({"n": 1.5}), MAX),
            Err(SpaceError::Decode(_))
        ));
        assert!(matches!(
            encode_record(&json!({"$link": "not-a-cid"}), MAX),
            Err(SpaceError::Decode(_))
        ));
        assert!(matches!(
            encode_record(&json!({"$bytes": "!!!"}), MAX),
            Err(SpaceError::Decode(_))
        ));
        assert!(matches!(
            encode_record(&json!({"text": "x".repeat(100)}), 16),
            Err(SpaceError::RecordTooLarge { .. })
        ));
    }

    #[test]
    fn multi_key_dollar_maps_stay_maps() {
        let value = json!({"m": {"$link": "bafyreia", "extra": 1}});
        let bytes = encode_record(&value, MAX).unwrap();
        assert_eq!(decode_record(&bytes).unwrap(), value);
    }

    #[test]
    fn decode_rejects_floats_and_malformed_cbor() {
        let float = serde_ipld_dagcbor::to_vec(&Ipld::Float(1.5)).unwrap();
        assert!(matches!(decode_record(&float), Err(SpaceError::Decode(_))));
        assert!(matches!(decode_record(&[0xff]), Err(SpaceError::Decode(_))));
    }
}
