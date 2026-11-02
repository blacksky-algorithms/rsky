//! STAR-lite v1 archives and the DRISL subset they carry (spec §Repo
//! serialization, §Space meta archive).
//!
//! A STAR file is a magic header, a DRISL metadata item, then a sequence of
//! `len | path | len | value` pairs with strictly increasing keys:
//!
//! ```text
//! [ magic | len | metadata ] [ len | path | len | record ] [ len | path | len | record ]...
//! ```

use crate::error::SpaceError;
use serde_json::Value;

pub const STAR_MAGIC: [u8; 4] = [0x2A, 0x6C, 0x01, 0x00];
pub const PROFILE_SPACE_REPO: &str = "atproto-space-repo";
pub const PROFILE_SPACE_META: &str = "atproto-space-meta";

/// A parsed STAR archive: the decoded metadata item and the raw body pairs
/// (values stay encoded; each profile defines their shape).
#[derive(Debug, Clone, PartialEq)]
pub struct StarArchive {
    pub metadata: Drisl,
    pub pairs: Vec<(String, Vec<u8>)>,
}

fn err(msg: impl Into<String>) -> SpaceError {
    SpaceError::Decode(msg.into())
}

fn write_varint(mut n: u64, out: &mut Vec<u8>) {
    loop {
        let byte = (n & 0x7f) as u8;
        n >>= 7;
        if n == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn read_varint(bytes: &[u8], pos: &mut usize) -> Result<u64, SpaceError> {
    let (mut value, mut shift) = (0u64, 0u32);
    loop {
        let byte = *bytes.get(*pos).ok_or_else(|| err("truncated varint"))?;
        *pos += 1;
        value |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
        if shift >= 64 {
            return Err(err("varint too long"));
        }
    }
}

/// The DRISL value subset space archives need: nulls, bools, ints, byte
/// strings, text, arrays, and maps with canonical (length-then-bytewise) key
/// order. Floating point and tags are rejected: nothing the profiles carry
/// can express them.
#[derive(Debug, Clone, PartialEq)]
pub enum Drisl {
    Null,
    Bool(bool),
    Int(i64),
    Bytes(Vec<u8>),
    Str(String),
    Array(Vec<Drisl>),
    /// Kept in canonical order by the encoder; decoders preserve read order.
    Map(Vec<(String, Drisl)>),
}

impl Drisl {
    /// JSON carries no byte strings, so `Bytes` only enters via decoding.
    pub fn from_json(value: &Value) -> Result<Self, SpaceError> {
        Ok(match value {
            Value::Null => Drisl::Null,
            Value::Bool(b) => Drisl::Bool(*b),
            Value::Number(n) => Drisl::Int(
                n.as_i64()
                    .ok_or_else(|| err("non-integer numbers are not drisl"))?,
            ),
            Value::String(s) => Drisl::Str(s.clone()),
            Value::Array(items) => Drisl::Array(
                items
                    .iter()
                    .map(Drisl::from_json)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            Value::Object(map) => Drisl::Map(
                map.iter()
                    .map(|(k, v)| Drisl::from_json(v).map(|d| (k.clone(), d)))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
        })
    }

    /// Lossy view back as JSON; byte strings become arrays of integers, which
    /// only ever matters for tests and logging.
    pub fn to_json(&self) -> Value {
        match self {
            Drisl::Null => Value::Null,
            Drisl::Bool(b) => Value::Bool(*b),
            Drisl::Int(i) => Value::Number((*i).into()),
            Drisl::Bytes(b) => Value::Array(b.iter().map(|b| Value::Number((*b).into())).collect()),
            Drisl::Str(s) => Value::String(s.clone()),
            Drisl::Array(items) => Value::Array(items.iter().map(Drisl::to_json).collect()),
            Drisl::Map(entries) => Value::Object(
                entries
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_json()))
                    .collect(),
            ),
        }
    }

    pub fn get(&self, key: &str) -> Option<&Drisl> {
        match self {
            Drisl::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Drisl::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Drisl::Int(i) => Some(*i),
            _ => None,
        }
    }

    fn encode_head(major: u8, len: u64, out: &mut Vec<u8>) {
        let head = major << 5;
        match len {
            0..=23 => out.push(head | len as u8),
            24..=0xff => out.extend([head | 24, len as u8]),
            0x100..=0xffff => out.extend([head | 25, (len >> 8) as u8, len as u8]),
            _ => {
                out.push(head | 27);
                out.extend(len.to_be_bytes());
            }
        }
    }

    /// Canonical encoding: map keys sort by encoded length then bytes.
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Drisl::Null => out.push(0xf6),
            Drisl::Bool(b) => out.push(if *b { 0xf5 } else { 0xf4 }),
            Drisl::Int(i) if *i >= 0 => Self::encode_head(0, *i as u64, out),
            Drisl::Int(i) => Self::encode_head(1, (-1 - *i) as u64, out),
            Drisl::Bytes(b) => {
                Self::encode_head(2, b.len() as u64, out);
                out.extend(b);
            }
            Drisl::Str(s) => {
                Self::encode_head(3, s.len() as u64, out);
                out.extend(s.as_bytes());
            }
            Drisl::Array(items) => {
                Self::encode_head(4, items.len() as u64, out);
                for item in items {
                    item.encode(out);
                }
            }
            Drisl::Map(entries) => {
                let mut entries = entries.clone();
                entries.sort_by(|a, b| (a.0.len(), &a.0).cmp(&(b.0.len(), &b.0)));
                Self::encode_head(5, entries.len() as u64, out);
                for (key, value) in entries {
                    Self::encode_head(3, key.len() as u64, out);
                    out.extend(key.as_bytes());
                    value.encode(out);
                }
            }
        }
    }

    pub fn encode_to_vec(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode(&mut out);
        out
    }

    /// Decode one item at `pos`, returning it and the position just past it.
    pub fn decode(bytes: &[u8], pos: &mut usize) -> Result<Drisl, SpaceError> {
        let initial = *bytes.get(*pos).ok_or_else(|| err("truncated drisl"))?;
        *pos += 1;
        let (major, info) = (initial >> 5, initial & 0x1f);
        let len = |pos: &mut usize| -> Result<u64, SpaceError> {
            Ok(match info {
                0..=23 => info as u64,
                24 => {
                    let n = *bytes.get(*pos).ok_or_else(|| err("truncated drisl"))?;
                    *pos += 1;
                    n as u64
                }
                25 => {
                    let n = bytes
                        .get(*pos..*pos + 2)
                        .ok_or_else(|| err("truncated drisl"))?;
                    *pos += 2;
                    u16::from_be_bytes([n[0], n[1]]) as u64
                }
                26 => {
                    let n = bytes
                        .get(*pos..*pos + 4)
                        .ok_or_else(|| err("truncated drisl"))?;
                    *pos += 4;
                    u32::from_be_bytes([n[0], n[1], n[2], n[3]]) as u64
                }
                27 => {
                    let n = bytes
                        .get(*pos..*pos + 8)
                        .ok_or_else(|| err("truncated drisl"))?;
                    *pos += 8;
                    u64::from_be_bytes(n.try_into().unwrap())
                }
                _ => return Err(err("indefinite-length drisl")),
            })
        };
        let take = |n: u64, pos: &mut usize| -> Result<&[u8], SpaceError> {
            let end = pos
                .checked_add(n as usize)
                .ok_or_else(|| err("drisl length overflow"))?;
            let slice = bytes.get(*pos..end).ok_or_else(|| err("truncated drisl"))?;
            *pos = end;
            Ok(slice)
        };
        Ok(match major {
            0 => Drisl::Int(len(pos)? as i64),
            1 => Drisl::Int(-1 - len(pos)? as i64),
            2 => Drisl::Bytes(take(len(pos)?, pos)?.to_vec()),
            3 => {
                let raw = take(len(pos)?, pos)?;
                Drisl::Str(
                    std::str::from_utf8(raw)
                        .map_err(|_| err("invalid utf-8 in drisl text"))?
                        .to_string(),
                )
            }
            4 => {
                let n = len(pos)?;
                let mut items = Vec::with_capacity(n.min(1 << 20) as usize);
                for _ in 0..n {
                    items.push(Drisl::decode(bytes, pos)?);
                }
                Drisl::Array(items)
            }
            5 => {
                let n = len(pos)?;
                let mut entries = Vec::with_capacity(n.min(1 << 20) as usize);
                for _ in 0..n {
                    let key = match Drisl::decode(bytes, pos)? {
                        Drisl::Str(s) => s,
                        _ => return Err(err("non-text drisl map key")),
                    };
                    entries.push((key, Drisl::decode(bytes, pos)?));
                }
                Drisl::Map(entries)
            }
            7 if info == 20 => Drisl::Bool(false),
            7 if info == 21 => Drisl::Bool(true),
            7 if info == 22 => Drisl::Null,
            _ => return Err(err("unsupported drisl item")),
        })
    }
}

/// Serialize a STAR archive. `pairs` must already be in strict lexicographic
/// order of key bytes with no duplicates (callers sort).
pub fn encode_star(metadata: &Drisl, pairs: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(64);
    out.extend(STAR_MAGIC);
    let meta = metadata.encode_to_vec();
    write_varint(meta.len() as u64, &mut out);
    out.extend(meta);
    for (key, value) in pairs {
        write_varint(key.len() as u64, &mut out);
        out.extend(key.as_bytes());
        write_varint(value.len() as u64, &mut out);
        out.extend(value);
    }
    out
}

/// Parse a STAR archive. Errors on a bad magic, malformed item, non-text
/// metadata, duplicate or out-of-order key, or trailing bytes.
pub fn decode_star(bytes: &[u8]) -> Result<StarArchive, SpaceError> {
    if bytes.len() < 4 || bytes[..4] != STAR_MAGIC {
        return Err(err("bad star magic"));
    }
    let mut pos = 4usize;
    let meta_len = read_varint(bytes, &mut pos)? as usize;
    let end = pos
        .checked_add(meta_len)
        .ok_or_else(|| err("metadata length overflow"))?;
    let meta_bytes = bytes
        .get(pos..end)
        .ok_or_else(|| err("truncated metadata"))?;
    let mut inner = 0usize;
    let metadata = Drisl::decode(meta_bytes, &mut inner)?;
    if inner != meta_bytes.len() {
        return Err(err("trailing bytes in metadata"));
    }
    pos = end;
    let mut pairs: Vec<(String, Vec<u8>)> = Vec::new();
    while pos < bytes.len() {
        let key_len = read_varint(bytes, &mut pos)? as usize;
        let end = pos
            .checked_add(key_len)
            .ok_or_else(|| err("key length overflow"))?;
        let key_bytes = bytes.get(pos..end).ok_or_else(|| err("truncated key"))?;
        let key = std::str::from_utf8(key_bytes)
            .map_err(|_| err("invalid utf-8 key"))?
            .to_string();
        pos = end;
        let value_len = read_varint(bytes, &mut pos)? as usize;
        let end = pos
            .checked_add(value_len)
            .ok_or_else(|| err("value length overflow"))?;
        let value = bytes
            .get(pos..end)
            .ok_or_else(|| err("truncated value"))?
            .to_vec();
        pos = end;
        if let Some((last, _)) = pairs.last() {
            if key.as_str() <= last.as_str() {
                return Err(err("star keys out of order or duplicated"));
            }
        }
        pairs.push((key, value));
    }
    Ok(StarArchive { metadata, pairs })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drisl_roundtrip_and_canonical_order() {
        let meta = Drisl::from_json(&serde_json::json!({
            "$type": "atproto-space-meta",
            "ver": 1,
            "space": "at://did:plc:a/space/com.example.f/x"
        }))
        .unwrap();
        let bytes = meta.encode_to_vec();
        // canonical order: "$type" (5) < "ver" (3)? No — (len,bytes): "ver"(3),
        // "$type"(5), "space"(5 -> "$" < "s").
        assert_eq!(&bytes[..7], &[0xa3, 0x63, b'v', b'e', b'r', 0x01, 0x65]);
        let mut pos = 0;
        let decoded = Drisl::decode(&bytes, &mut pos).unwrap();
        assert_eq!(decoded.get("ver"), Some(&Drisl::Int(1)));
        assert_eq!(
            decoded.get("$type"),
            Some(&Drisl::Str("atproto-space-meta".to_string()))
        );
        assert!(matches!(decoded.get("space"), Some(Drisl::Str(_))));
        assert_eq!(pos, bytes.len());
    }

    #[test]
    fn star_roundtrip_with_bytes_values() {
        let pairs = vec![
            ("a".to_string(), vec![0xf4]),
            ("writer/did:plc:x".to_string(), vec![1, 2, 3]),
        ];
        let archive = decode_star(&encode_star(&Drisl::Null, &pairs)).unwrap();
        assert_eq!(archive.metadata, Drisl::Null);
        assert_eq!(archive.pairs, pairs);
    }

    #[test]
    fn star_rejects_out_of_order_and_bad_magic() {
        let pairs = vec![("b".to_string(), vec![0]), ("a".to_string(), vec![0])];
        assert!(decode_star(&encode_star(&Drisl::Null, &pairs)).is_err());
        assert!(decode_star(b"\x2a\x6c\x01").is_err());
        assert!(decode_star(&encode_star(&Drisl::Null, &pairs)[..5]).is_err());
    }
}
