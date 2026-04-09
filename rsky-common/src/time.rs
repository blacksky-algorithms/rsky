use crate::RFC3339_VARIANT;
use anyhow::{anyhow, Result};
use chrono::offset::Utc as UtcOffset;
use chrono::{DateTime, NaiveDateTime, Utc};
use std::time::SystemTime;

pub const SECOND: i32 = 1000;
pub const MINUTE: i32 = SECOND * 60;
pub const HOUR: i32 = MINUTE * 60;
pub const DAY: i32 = HOUR * 24;

pub fn less_than_ago_s(time: DateTime<UtcOffset>, range: i32) -> bool {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("timestamp in micros since UNIX epoch")
        .as_secs() as usize;
    let x = time.timestamp() as usize + range as usize;
    now < x
}

/// Parse a datetime string to microseconds since epoch.
/// Tries the primary RFC3339 variant format first, then falls back to
/// full RFC 3339 parsing (handles `+00:00` offsets and other variants).
pub fn from_str_to_micros(str: &str) -> Result<i64> {
    if let Ok(dt) = NaiveDateTime::parse_from_str(str, RFC3339_VARIANT) {
        return Ok(dt.and_utc().timestamp_micros());
    }
    DateTime::parse_from_rfc3339(str)
        .map(|dt| dt.timestamp_micros())
        .map_err(|e| anyhow!("failed to parse datetime {:?}: {}", str, e))
}

pub fn from_str_to_millis(str: &String) -> Result<i64> {
    Ok(NaiveDateTime::parse_from_str(str, RFC3339_VARIANT)?
        .and_utc()
        .timestamp_millis())
}

/// Parse a datetime string to a UTC DateTime.
/// Tries the primary RFC3339 variant format first, then falls back to
/// full RFC 3339 parsing (handles `+00:00` offsets and other variants).
pub fn from_str_to_utc(str: &str) -> Result<DateTime<UtcOffset>> {
    if let Ok(dt) = NaiveDateTime::parse_from_str(str, RFC3339_VARIANT) {
        return Ok(dt.and_utc());
    }
    DateTime::parse_from_rfc3339(str)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| anyhow!("failed to parse datetime {:?}: {}", str, e))
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

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_PRIMARY: &str = "2023-11-14T22:13:20.000Z";
    const SAMPLE_OFFSET: &str = "2023-11-14T22:13:20+00:00";

    #[test]
    fn from_str_to_micros_parses_primary_format() {
        assert!(from_str_to_micros(SAMPLE_PRIMARY).is_ok());
    }

    #[test]
    fn from_str_to_micros_parses_rfc3339_with_offset() {
        // Fallback: RFC 3339 with +00:00 offset — same instant as Z suffix
        let with_z = from_str_to_micros(SAMPLE_PRIMARY).unwrap();
        let with_offset = from_str_to_micros(SAMPLE_OFFSET).unwrap();
        assert_eq!(with_z, with_offset);
    }

    #[test]
    fn from_str_to_micros_returns_err_on_invalid_input() {
        assert!(from_str_to_micros("not-a-date").is_err());
        assert!(from_str_to_micros("").is_err());
    }

    #[test]
    fn from_str_to_utc_parses_primary_format() {
        assert!(from_str_to_utc(SAMPLE_PRIMARY).is_ok());
    }

    #[test]
    fn from_str_to_utc_parses_rfc3339_with_offset() {
        // Both formats should resolve to the same UTC instant
        let with_z = from_str_to_utc(SAMPLE_PRIMARY).unwrap();
        let with_offset = from_str_to_utc(SAMPLE_OFFSET).unwrap();
        assert_eq!(with_z, with_offset);
    }

    #[test]
    fn from_str_to_utc_returns_err_on_invalid_input() {
        assert!(from_str_to_utc("not-a-date").is_err());
        assert!(from_str_to_utc("").is_err());
    }
}
