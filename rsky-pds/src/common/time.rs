use crate::common::RFC3339_VARIANT;
use anyhow::Result;
use chrono::offset::Utc as UtcOffset;
use chrono::{DateTime, NaiveDateTime, Utc};
use std::time::SystemTime;

pub const SECOND: i32 = 1000;
pub const MINUTE: i32 = SECOND * 60;
pub const HOUR: i32 = MINUTE * 60;
pub const DAY: i32 = HOUR * 24;

pub fn less_than_ago_ms(time: DateTime<UtcOffset>, range: i32) -> bool {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("timestamp in micros since UNIX epoch")
        .as_micros() as usize;
    now < (time.timestamp() as usize + range as usize)
}

pub fn from_str_to_micros(str: &String) -> i64 {
    NaiveDateTime::parse_from_str(str, RFC3339_VARIANT)
        .unwrap()
        .and_utc()
        .timestamp_micros()
}

pub fn from_str_to_millis(str: &String) -> Result<i64> {
    Ok(NaiveDateTime::parse_from_str(str, RFC3339_VARIANT)?
        .and_utc()
        .timestamp_millis())
}

pub fn from_str_to_utc(str: &String) -> DateTime<UtcOffset> {
    NaiveDateTime::parse_from_str(str, RFC3339_VARIANT)
        .unwrap()
        .and_utc()
}

#[allow(deprecated)]
pub fn from_micros_to_utc(micros: i64) -> DateTime<UtcOffset> {
    let nanoseconds = 230 * 1000000;
    DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(micros, nanoseconds), Utc)
}

pub fn from_micros_to_str(micros: i64) -> String {
    format!("{}", from_micros_to_utc(micros).format(RFC3339_VARIANT))
}

pub fn from_millis_to_utc(millis: i64) -> DateTime<UtcOffset> {
    DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp_millis(millis).unwrap(), Utc)
}

pub fn from_millis_to_str(millis: i64) -> String {
    format!("{}", from_millis_to_utc(millis).format(RFC3339_VARIANT))
}
