use chrono::{DateTime, Datelike, FixedOffset, NaiveDateTime, Utc};
use lazy_static::lazy_static;
use regex::Regex;
use thiserror::Error;

/* Validates datetime string against atproto Lexicon 'datetime' format.
 * Syntax is described at: https://atproto.com/specs/lexicon#datetime
 */

lazy_static! {
    static ref DATETIME_REGEX: Regex = Regex::new(
        r"^[0-9]{4}-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-5][0-9](.[0-9]{1,20})?(Z|([+-][0-2][0-9]:[0-5][0-9]))$"
    ).unwrap();
}

#[derive(Error, Debug)]
#[error("InvalidDatetimeError: {0}")]
pub struct InvalidDatetimeError(String);

pub fn ensure_valid_datetime<S: Into<String>>(
    dt_str: S,
) -> Result<DateTime<FixedOffset>, InvalidDatetimeError> {
    let dt_str: String = dt_str.into();
    // Regex check first - this validates the basic format
    if !DATETIME_REGEX.is_match(&dt_str) {
        return Err(InvalidDatetimeError(
            "datetime didn't validate via regex".into(),
        ));
    }

    // Check for negative dates and year zero
    if dt_str.starts_with('-') {
        return Err(InvalidDatetimeError(
            "datetime normalized to a negative time".into(),
        ));
    }

    if dt_str.starts_with("000") {
        return Err(InvalidDatetimeError(
            "datetime so close to year zero not allowed".into(),
        ));
    }

    // Must parse as ISO 8601; this also verifies semantics like month is not 13 or 00
    let parsed = DateTime::parse_from_rfc3339(&dt_str)
        .map_err(|_| InvalidDatetimeError("datetime did not parse as ISO 8601".into()))?;

    if dt_str.len() > 64 {
        return Err(InvalidDatetimeError(
            "datetime is too long (64 chars max)".into(),
        ));
    }

    if dt_str.ends_with("-00:00") {
        return Err(InvalidDatetimeError(
            "datetime can not use \"-00:00\" for UTC timezone".into(),
        ));
    }

    // Additional validation: the datetime must be valid according to the calendar
    if parsed.year() < 1 {
        return Err(InvalidDatetimeError(
            "datetime before year 1 not allowed".into(),
        ));
    }

    Ok(parsed)
}

pub fn is_valid_datetime<S: Into<String>>(dt_str: S) -> bool {
    ensure_valid_datetime(dt_str).is_ok()
}

/* Takes a flexible datetime string and normalizes representation.
 *
 * This function will work with any valid atproto datetime (eg, anything which is_valid_datetime() is true for).
 * It *additionally* is more flexible about accepting datetimes that don't comply to RFC 3339,
 * or are missing timezone information, and normalizing them to a valid datetime.
 *
 * One use-case is a consistent, sortable string. Another is to work with older invalid createdAt datetimes.
 *
 * Successful output will be a valid atproto datetime with millisecond precision (3 sub-second digits)
 * and UTC timezone with trailing 'Z' syntax. Throws `InvalidDatetimeError` if the input string could
 * not be parsed as a datetime, even with permissive parsing.
 *
 * Expected output format: YYYY-MM-DDTHH:mm:ss.sssZ
 */
pub fn normalize_datetime<S: Into<String>>(dt_str: S) -> Result<String, InvalidDatetimeError> {
    let dt_str: String = dt_str.into();
    // First try strict RFC3339/ISO8601 parsing
    if let Ok(dt) = DateTime::parse_from_rfc3339(&dt_str) {
        let utc_dt = dt.with_timezone(&Utc);
        let formatted = utc_dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        if is_valid_datetime(&formatted) {
            return Ok(formatted);
        }
    }

    // Special case for timestamps with milliseconds but no timezone
    if let Ok(naive_dt) = NaiveDateTime::parse_from_str(&dt_str, "%Y-%m-%dT%H:%M:%S%.3f") {
        let utc_dt = DateTime::<Utc>::from_naive_utc_and_offset(naive_dt, Utc);
        let formatted = utc_dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        if is_valid_datetime(&formatted) {
            return Ok(formatted);
        }
    }

    // If no timezone specified, try parsing as naive datetime and then convert to UTC
    if !dt_str.ends_with('Z') && !dt_str.contains('+') && !dt_str.contains('-') {
        if let Ok(naive_dt) = NaiveDateTime::parse_from_str(&dt_str, "%Y-%m-%dT%H:%M:%S") {
            let utc_dt = DateTime::<Utc>::from_naive_utc_and_offset(naive_dt, Utc);
            let formatted = utc_dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
            if is_valid_datetime(&formatted) {
                return Ok(formatted);
            }
        }
    }

    // Try other flexible parsing formats
    let parsed = DateTime::parse_from_rfc3339(&dt_str)
        .or_else(|_| DateTime::parse_from_str(&dt_str, "%Y-%m-%dT%H:%M:%S%.f%:z"))
        .or_else(|_| DateTime::parse_from_str(&dt_str, "%Y-%m-%d %H:%M:%S%.f%:z"))
        // Try multiple variations of the RFC 822/RFC 2822 date format
        .or_else(|_| DateTime::parse_from_str(&dt_str, "%a, %d %b %Y %H:%M:%S GMT"))
        .or_else(|_| DateTime::parse_from_str(&dt_str, "%a, %d %B %Y %H:%M:%S GMT")) // Full month name
        .or_else(|_| DateTime::parse_from_str(&dt_str, "%A, %d %b %Y %H:%M:%S GMT")) // Full weekday
        .or_else(|_| DateTime::parse_from_str(&dt_str, "%A, %d %B %Y %H:%M:%S GMT")) // Both full
        // Try with %Z
        .or_else(|_| DateTime::parse_from_str(&dt_str, "%a, %d %b %Y %H:%M:%S %Z"))
        .or_else(|_| DateTime::parse_from_str(&dt_str, "%a, %d %B %Y %H:%M:%S %Z")) // Full month name
        .or_else(|_| DateTime::parse_from_str(&dt_str, "%A, %d %b %Y %H:%M:%S %Z")) // Full weekday
        .or_else(|_| DateTime::parse_from_str(&dt_str, "%A, %d %B %Y %H:%M:%S %Z")) // Both full
        // Try multiple variations of the RFC 822/RFC 2822 date format
        .or_else(|_| DateTime::parse_from_rfc2822(&dt_str)); // last chance to parse format

    match parsed {
        Ok(dt) => {
            let utc_dt = dt.with_timezone(&Utc);
            let formatted = utc_dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
            if is_valid_datetime(&formatted) {
                Ok(formatted)
            } else {
                Err(InvalidDatetimeError(
                    "datetime normalized to invalid timestamp string".into(),
                ))
            }
        }
        Err(e) => {
            let kind_error = format!(
                "datetime did not parse as any timestamp format, ParseErrorKind: {:?}",
                e.kind()
            );
            Err(InvalidDatetimeError(kind_error))
        }
    }
}

/* Variant of normalizeDatetime() which always returns a valid datetime string.
 *
 * If a InvalidDatetimeError is encountered, returns the UNIX epoch time as a UTC datetime (1970-01-01T00:00:00.000Z).
 */
pub fn normalize_datetime_always<S: Into<String>>(dt_str: S) -> String {
    match normalize_datetime(dt_str) {
        Ok(dt) => dt,
        Err(_) => "1970-01-01T00:00:00.000Z".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expect_valid(dt: &str) {
        ensure_valid_datetime(dt)
            .unwrap_or_else(|e| panic!("Expected '{}' to be valid, but got error: {}", dt, e));
        assert!(is_valid_datetime(dt), "Expected '{}' to be valid", dt);
        normalize_datetime(dt)
            .unwrap_or_else(|e| panic!("Expected '{}' to normalize, but got error: {}", dt, e));
        normalize_datetime_always(dt);
    }

    fn expect_invalid(dt: &str) {
        assert!(
            ensure_valid_datetime(dt).is_err(),
            "Expected '{}' to be invalid, but it was considered valid",
            dt
        );
        assert!(
            !is_valid_datetime(dt),
            "Expected '{}' to be invalid, but is_valid_datetime returned true",
            dt
        );
    }

    #[test]
    fn test_valid_datetimes() {
        expect_valid("2023-01-01T00:00:00Z");
        expect_valid("2023-01-01T00:00:00.000Z");
        expect_valid("2023-01-01T00:00:00.123Z");
        expect_valid("2023-01-01T00:00:00+00:00");
        expect_valid("2023-01-01T00:00:00-07:00");
        expect_valid("2023-01-01T00:00:00.123456789Z");
        expect_valid("9999-12-31T23:59:59.999Z");
        // from documentation
        // preferred
        expect_valid("1985-04-12T23:20:50.123Z");
        expect_valid("1985-04-12T23:20:50.123456Z");
        expect_valid("1985-04-12T23:20:50.120Z");
        expect_valid("1985-04-12T23:20:50.120000Z");

        // supported
        expect_valid("1985-04-12T23:20:50.12345678912345Z");
        expect_valid("1985-04-12T23:20:50Z");
        expect_valid("1985-04-12T23:20:50.0Z");
        expect_valid("1985-04-12T23:20:50.123+00:00");
        expect_valid("1985-04-12T23:20:50.123-07:00");
    }

    #[test]
    fn test_invalid_datetimes() {
        expect_invalid("");
        expect_invalid("2023");
        expect_invalid("2023-01-01");
        expect_invalid("2023-01-01T");
        expect_invalid("2023-01-01T00:00:00");
        expect_invalid("2023-01-01 00:00:00Z");
        expect_invalid("2023-13-01T00:00:00Z");
        expect_invalid("2023-00-01T00:00:00Z");
        expect_invalid("2023-01-32T00:00:00Z");
        expect_invalid("2023-01-01T24:00:00Z");
        expect_invalid("2023-01-01T00:60:00Z");
        expect_invalid("2023-01-01T00:00:60Z");
        expect_invalid("-2023-01-01T00:00:00Z");
        expect_invalid("0000-01-01T00:00:00Z");
        expect_invalid("2023-01-01T00:00:00-00:00");
        expect_invalid(&format!("2023-01-01T00:00:00{}", "0".repeat(65)));
        expect_invalid("0000-01-01T00:00:00.000Z");
        // Documentation invalid examples
        expect_invalid("1985-04-12");
        expect_invalid("1985-04-12T23:20Z");
        expect_invalid("1985-04-12T23:20:5Z");
        expect_invalid("1985-04-12T23:20:50.123");
        expect_invalid("+001985-04-12T23:20:50.123Z");
        expect_invalid("23:20:50.123Z");
        expect_invalid("-1985-04-12T23:20:50.123Z");
        expect_invalid("1985-4-12T23:20:50.123Z");
        expect_invalid("01985-04-12T23:20:50.123Z");
        expect_invalid("1985-04-12T23:20:50.123+00");
        expect_invalid("1985-04-12T23:20:50.123+0000");

        //ISO-8601 strict capitalization
        expect_invalid("1985-04-12t23:20:50.123Z");
        expect_invalid("1985-04-12T23:20:50.123z");

        // RFC-3339, but not ISO-8601
        expect_invalid("1985-04-12T23:20:50.123-00:00");
        expect_invalid("1985-04-12 23:20:50.123Z");

        // timezone is required
        expect_invalid("1985-04-12T23:20:50.123");

        // syntax looks ok, but datetime is not valid
        expect_invalid("1985-04-12T23:99:50.123Z");
        expect_invalid("1985-00-12T23:20:50.123Z");
    }

    #[test]
    fn test_datetime_normalization() {
        // Test basic normalization
        assert_eq!(
            normalize_datetime("1234-04-12T23:20:50Z").unwrap(),
            "1234-04-12T23:20:50.000Z"
        );
        assert_eq!(
            normalize_datetime("1985-04-12T23:20:50Z").unwrap(),
            "1985-04-12T23:20:50.000Z"
        );
        assert_eq!(
            normalize_datetime("1985-04-12T23:20:50.123").unwrap(),
            "1985-04-12T23:20:50.123Z"
        );

        // Test timezone conversion
        assert_eq!(
            normalize_datetime("1985-04-12T10:20:50.1+01:00").unwrap(),
            "1985-04-12T09:20:50.100Z"
        );

        // Test alternative formats
        assert_eq!(
            // The original typescript test was Fri, 02 Jan 1999 12:34:56 GMT
            // however, January 2, 1999 is actually a Saturday
            normalize_datetime("Sat, 02 Jan 1999 12:34:56 GMT").unwrap(),
            "1999-01-02T12:34:56.000Z"
        );
    }

    #[test]
    fn test_invalid_normalization() {
        assert!(normalize_datetime("").is_err());
        assert!(normalize_datetime("blah").is_err());
        assert!(normalize_datetime("1999-19-39T23:20:50.123Z").is_err());
        assert!(normalize_datetime("-000001-12-31T23:00:00.000Z").is_err());
        assert!(normalize_datetime("0000-01-01T00:00:00+01:00").is_err());
    }

    #[test]
    fn test_normalize_always() {
        assert_eq!(
            normalize_datetime_always("1985-04-12T23:20:50Z"),
            "1985-04-12T23:20:50.000Z"
        );
        assert_eq!(
            normalize_datetime_always("blah"),
            "1970-01-01T00:00:00.000Z"
        );
        assert_eq!(
            normalize_datetime_always("0000-01-01T00:00:00+01:00"),
            "1970-01-01T00:00:00.000Z"
        );
    }
}
