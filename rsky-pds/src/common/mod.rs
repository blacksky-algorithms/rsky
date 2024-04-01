use anyhow::Result;
use base64ct::{Base64, Encoding};
use chrono::offset::Utc as UtcOffset;
use chrono::DateTime;
use indexmap::IndexMap;
use rand::{distributions::Alphanumeric, Rng};
use serde::Serialize;
use serde_json::Value;
use std::time::SystemTime;
use url::form_urlencoded;

pub const RFC3339_VARIANT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";

pub fn now() -> String {
    let system_time = SystemTime::now();
    let dt: DateTime<UtcOffset> = system_time.into();
    format!("{}", dt.format(RFC3339_VARIANT))
}

pub fn get_random_str() -> String {
    let token: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();
    token
}

pub fn struct_to_cbor<T: Serialize>(obj: T) -> Result<Vec<u8>> {
    // Encode object to json before dag-cbor because serde_ipld_dagcbor doesn't properly
    // sort by keys
    let json = serde_json::to_string(&obj)?;
    // Deserialize to IndexMap with preserve key order enabled. serde_ipld_dagcbor does not sort nested
    // objects properly by keys
    let map: IndexMap<String, Value> = serde_json::from_str(&json)?;
    let cbor_bytes = serde_ipld_dagcbor::to_vec(&map)?;

    Ok(cbor_bytes)
}

pub fn json_to_b64url<T: Serialize>(obj: &T) -> Result<String> {
    Ok(Base64::encode_string((&serde_json::to_string(obj)?).as_ref()).replace("=", ""))
}

pub fn encode_uri_component(input: &String) -> String {
    form_urlencoded::byte_serialize(input.as_bytes()).collect()
}

pub mod env;
pub mod ipld;
pub mod tid;
pub mod time;
