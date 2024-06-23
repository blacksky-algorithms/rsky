use anyhow::Result;
use urlencoding::{decode, encode};

pub const SECOND: i32 = 1000;
pub const MINUTE: i32 = SECOND * 60;
pub const HOUR: i32 = MINUTE * 60;
pub const DAY: i32 = HOUR * 24;

pub fn encode_uri_component(input: &String) -> String {
    encode(input).to_string()
}
pub fn decode_uri_component(input: &str) -> Result<String> {
    Ok(decode(input)?.to_string())
}
