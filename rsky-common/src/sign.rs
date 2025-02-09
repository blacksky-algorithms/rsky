use anyhow::Result;
use indexmap::IndexMap;
use secp256k1::{Message, SecretKey};
use serde::Serialize;
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};

pub fn atproto_sign<T: Serialize>(obj: &T, key: &SecretKey) -> Result<[u8; 64]> {
    // Encode object to json before dag-cbor because serde_ipld_dagcbor doesn't properly
    // sort by keys
    let json = serde_json::to_string(obj).unwrap();
    // Deserialize to IndexMap with preserve key order enabled. serde_ipld_dagcbor does not sort nested
    // objects properly by keys
    let map_unsigned: IndexMap<String, JsonValue> = serde_json::from_str(&json).unwrap();
    let unsigned_bytes = serde_ipld_dagcbor::to_vec(&map_unsigned).unwrap();
    // Hash dag_cbor to sha256
    let hash = Sha256::digest(&*unsigned_bytes);
    // Sign sha256 hash using private key
    let message = Message::from_digest_slice(hash.as_ref()).unwrap();
    let mut sig = key.sign_ecdsa(message);
    // Convert to low-s
    sig.normalize_s();
    // ASN.1 encoded per decode_dss_signature
    let normalized_compact_sig = sig.serialize_compact();
    Ok(normalized_compact_sig)
}

pub fn sign_without_indexmap<T: Serialize>(obj: &T, key: &SecretKey) -> Result<[u8; 64]> {
    let unsigned_bytes = serde_ipld_dagcbor::to_vec(&obj)?;
    // Hash dag_cbor to sha256
    let hash = Sha256::digest(&*unsigned_bytes);
    // Sign sha256 hash using private key
    let message = Message::from_digest_slice(hash.as_ref())?;
    let mut sig = key.sign_ecdsa(message);
    // Convert to low-s
    sig.normalize_s();
    // ASN.1 encoded per decode_dss_signature
    let normalized_compact_sig = sig.serialize_compact();
    Ok(normalized_compact_sig)
}
