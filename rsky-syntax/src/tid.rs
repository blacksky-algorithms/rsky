use lazy_static::lazy_static;
use regex::Regex;
use thiserror::Error;

lazy_static! {
    // Reference regex for TID:
    // Must be exactly 13 characters from base32-sortable charset
    // First character must be one of 234567abcdefghij
    // All other characters from base32-sortable charset: 234567abcdefghijklmnopqrstuvwxyz
    static ref TID_REGEX: Regex = Regex::new(
        r"^[234567abcdefghij][234567abcdefghijklmnopqrstuvwxyz]{12}$"
    ).unwrap();
}

#[derive(Error, Debug)]
#[error("InvalidTidError: {0}")]
pub struct InvalidTidError(String);

/// Validates a TID string according to the atproto specification.
///
/// TID ("timestamp identifier") is a compact string identifier based on an integer timestamp.
/// The string must be:
/// - 64-bit integer based
/// - big-endian byte ordering
/// - encoded as base32-sortable (chars: 234567abcdefghijklmnopqrstuvwxyz)
/// - no padding characters
/// - length is always 13 ASCII characters
/// - first character must be one of 234567abcdefghij (indicating high bit is 0)
///
/// The integer layout is:
/// - top bit always 0
/// - next 53 bits are microseconds since UNIX epoch (for JavaScript safe integers)
/// - final 10 bits are a random "clock identifier"
pub fn ensure_valid_tid<S: Into<String>>(tid: S) -> Result<(), InvalidTidError> {
    let tid: String = tid.into();
    // Check fixed length
    if tid.len() != 13 {
        return Err(InvalidTidError("TID must be 13 characters".into()));
    }

    // Validate format using regex
    if !TID_REGEX.is_match(&tid) {
        return Err(InvalidTidError("TID syntax not valid (regex)".into()));
    }

    Ok(())
}

/// Returns true if the given string is a valid TID, false otherwise.
pub fn is_valid_tid<S: Into<String>>(tid: S) -> bool {
    ensure_valid_tid(tid).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expect_valid(tid: &str) {
        ensure_valid_tid(tid).unwrap();
        assert!(is_valid_tid(tid));
    }

    fn expect_invalid(tid: &str) {
        assert!(ensure_valid_tid(tid).is_err());
        assert!(!is_valid_tid(tid));
    }

    #[test]
    fn test_valid_tids() {
        // Test known valid TIDs
        expect_valid("3jzfcijpj2z2a");
        expect_valid("7777777777777");
        expect_valid("3zzzzzzzzzzzz");
        expect_valid("2222222222222");

        // Test various first characters
        for c in "234567abcdefghij".chars() {
            let tid = format!("{}222222222222", c);
            expect_valid(&tid);
        }
    }

    #[test]
    fn test_invalid_tids() {
        // Wrong length
        expect_invalid("3jzfcijpj2z2");
        expect_invalid("3jzfcijpj2z2aa");
        expect_invalid("");
        expect_invalid("222");

        // Invalid characters
        expect_invalid("3jzfcijpj2z21"); // contains '1'
        expect_invalid("0000000000000"); // contains '0'
        expect_invalid("3JZFCIJPJ2Z2A"); // uppercase

        // Legacy dash syntax not supported
        expect_invalid("3jzf-cij-pj2z-2a");

        // High bit can't be set (first char restrictions)
        expect_invalid("zzzzzzzzzzzzz");
        expect_invalid("kjzfcijpj2z2a");
        expect_invalid("lzzzzzzzzzzzz");
        expect_invalid("mzzzzzzzzzzzz");
        expect_invalid("nzzzzzzzzzzzz");
        expect_invalid("ozzzzzzzzzzzz");
        expect_invalid("pzzzzzzzzzzzz");
        expect_invalid("qzzzzzzzzzzzz");
        expect_invalid("rzzzzzzzzzzzz");
        expect_invalid("szzzzzzzzzzzz");
        expect_invalid("tzzzzzzzzzzzz");
        expect_invalid("uzzzzzzzzzzzz");
        expect_invalid("vzzzzzzzzzzzz");
        expect_invalid("wzzzzzzzzzzzz");
        expect_invalid("xzzzzzzzzzzzz");
        expect_invalid("yzzzzzzzzzzzz");
        expect_invalid("zzzzzzzzzzzzz");

        // Invalid characters in the rest of the string
        expect_invalid("3jzfcijpj2z2!");
        expect_invalid("3jzfcijpj2z2@");
        expect_invalid("3jzfcijpj2z2#");
        expect_invalid("3jzfcijpj2z2$");
        expect_invalid("3jzfcijpj2z2%");
        expect_invalid("3jzfcijpj2z2^");
        expect_invalid("3jzfcijpj2z2&");
        expect_invalid("3jzfcijpj2z2*");
        expect_invalid("3jzfcijpj2z2(");
        expect_invalid("3jzfcijpj2z2)");
        expect_invalid("3jzfcijpj2z2_");
        expect_invalid("3jzfcijpj2z2+");
        expect_invalid("3jzfcijpj2z2=");
        expect_invalid("3jzfcijpj2z2`");
        expect_invalid("3jzfcijpj2z2~");

        // Spaces and special characters
        expect_invalid(" 3jzfcijpj2z2");
        expect_invalid("3jzfcijpj2z2 ");
        expect_invalid("3jzf ijpj2z2a");
        expect_invalid("\t3jzfcijpj2z2");
        expect_invalid("3jzfcijpj2z2\n");
    }
}
