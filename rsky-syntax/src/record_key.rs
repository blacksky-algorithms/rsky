use lazy_static::lazy_static;
use regex::Regex;
use thiserror::Error;

lazy_static! {
    // Regex to validate record key syntax:
    // - ASCII alphanumeric chars, plus underscore, tilde, period, colon, hyphen
    // - Length between 1 and 512 characters
    static ref RKEY_REGEX: Regex = Regex::new(r"^[a-zA-Z0-9_~.:-]{1,512}$").unwrap();
}

#[derive(Error, Debug)]
#[error("InvalidRecordKeyError: {0}")]
pub struct InvalidRecordKeyError(String);

/// Validates a record key according to AT Protocol specification.
///
/// Record Keys constraints:
/// - Must contain only a subset of ASCII characters: alphanumeric (A-Za-z0-9),
///   period, dash, underscore, colon, or tilde (.-_:~)
/// - Must have at least 1 and at most 512 characters
/// - The specific values "." and ".." are not allowed
/// - Must be a permissible part of repository MST path string
/// - Must be permissible to include in a path component of a URI (following RFC-3986, section 3.3)
///
/// Record Keys are case-sensitive.
pub fn ensure_valid_record_key<S: Into<String>>(rkey: S) -> Result<(), InvalidRecordKeyError> {
    let rkey: String = rkey.into();
    if rkey.is_empty() || rkey.len() > 512 {
        return Err(InvalidRecordKeyError(
            "record key must be 1 to 512 characters".into(),
        ));
    }

    // Check for forbidden exact values
    if rkey == "." || rkey == ".." {
        return Err(InvalidRecordKeyError(
            "record key can not be \".\" or \"..\"".into(),
        ));
    }

    // Validate format using regex
    if !RKEY_REGEX.is_match(&rkey) {
        return Err(InvalidRecordKeyError(
            "record key syntax not valid (regex)".into(),
        ));
    }

    Ok(())
}

/// Returns true if the given string is a valid record key, false otherwise.
pub fn is_valid_record_key<S: Into<String>>(rkey: S) -> bool {
    ensure_valid_record_key(rkey).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expect_valid(rkey: &str) {
        ensure_valid_record_key(rkey).unwrap_or_else(|e| panic!("ensure_valid_record_key validation test: Expected '{}' to be valid, but got error: {}", rkey, e));
        assert!(
            is_valid_record_key(rkey),
            "is_valid_record_key validation error: Expected '{}' to be true, but got false",
            rkey
        );
    }

    fn expect_invalid(rkey: &str) {
        assert!(ensure_valid_record_key(rkey).is_err(), "ensure_valid_record_key invalidation test: Expected '{}' to be invalid, but it was considered valid", rkey);
        assert!(
            !is_valid_record_key(rkey),
            "is_valid_record_key invalidation test:  Expected '{}' to be false, but it was true",
            rkey
        );
    }

    #[test]
    fn test_valid_record_keys() {
        // Basic valid keys
        expect_valid("3jui7kd54zh2y");
        expect_valid("self");
        expect_valid("example.com");
        expect_valid("~1.2-3_");
        expect_valid("dHJ1ZQ");
        expect_valid("pre:fix");
        expect_valid("_");

        // Test allowed special characters
        expect_valid("test.record");
        expect_valid("test-record");
        expect_valid("test_record");
        expect_valid("test~record");
        expect_valid("test:record");

        // Test mixed case
        expect_valid("TestRecord");
        expect_valid("testRecord");
        expect_valid("TESTRECORD");

        // Test numbers
        expect_valid("123");
        expect_valid("test123");
        expect_valid("123test");

        // Test edge cases for length
        expect_valid("a");
        expect_valid(&"a".repeat(512));
    }

    #[test]
    fn test_invalid_record_keys() {
        // Invalid characters
        expect_invalid("alpha/beta");
        expect_invalid("@handle");
        expect_invalid("any space");
        expect_invalid("any+space");
        expect_invalid("number[3]");
        expect_invalid("number(3)");
        expect_invalid("\"quote\"");
        expect_invalid("test\\record");
        expect_invalid("test?record");
        expect_invalid("test#record");
        expect_invalid("test&record");
        expect_invalid("test%record");

        // Reserved names
        expect_invalid(".");
        expect_invalid("..");

        // Length violations
        expect_invalid("");
        expect_invalid(&"a".repeat(513));

        // Spaces and special characters
        expect_invalid(" test");
        expect_invalid("test ");
        expect_invalid("test test");
        expect_invalid("\ttest");
        expect_invalid("test\n");

        // Non-ASCII characters
        expect_invalid("tést");
        expect_invalid("テスト");
        expect_invalid("🔑");
    }

    #[test]
    fn test_edge_cases() {
        // Length boundary tests
        expect_invalid(""); // empty string
        expect_valid("a"); // minimum length (1)
        expect_valid(&"a".repeat(512)); // maximum length
        expect_invalid(&"a".repeat(513)); // exceeds maximum
        expect_valid(&"z".repeat(512)); // different char at max length
        expect_valid(&"9".repeat(512)); // numbers at max length

        // Special character combinations
        expect_valid("a~b_c.d:e-f"); // all allowed special chars
        expect_valid("~~~~~~~~~"); // all tildes
        expect_valid("........"); // multiple dots (but not . or ..)
        expect_valid("::::::::"); // all colons
        expect_valid("--------"); // all hyphens
        expect_valid("________"); // all underscores

        // Dots in various positions (but avoiding . and .. exactly)
        expect_valid("a."); // dot at end
        expect_valid(".a"); // dot at start
        expect_valid("a.a"); // single dot middle
        expect_valid("a..a"); // double dot middle
        expect_valid("...a"); // multiple dots start
        expect_valid("a..."); // multiple dots end

        // Almost . and .. cases
        expect_valid(".suffix"); // starts with dot
        expect_valid("prefix."); // ends with dot
        expect_valid("..suffix"); // starts with dots
        expect_valid("prefix.."); // ends with dots
        expect_invalid(".."); // exactly .. (invalid)
        expect_invalid("."); // exactly . (invalid)

        // Case sensitivity tests
        expect_valid("UPPER");
        expect_valid("lower");
        expect_valid("MiXeDcAsE");
        expect_valid("camelCase");
        expect_valid("PascalCase");
        expect_valid("snake_case");
        expect_valid("SCREAMING_SNAKE_CASE");
        expect_valid("kebab-case");
        expect_valid("UPPER-KEBAB-CASE");

        // Number patterns
        expect_valid("123"); // all numbers
        expect_valid("0"); // single zero
        expect_valid("000"); // multiple zeros
        expect_valid("0a"); // zero prefix
        expect_valid("a0"); // zero suffix
        expect_valid("99999999"); // many numbers

        // Mixed pattern tests
        expect_valid("a-b_c.d:e~f"); // alternating chars and special chars
        expect_valid("A-B_C.D:E~F"); // upper case version
        expect_valid("123-456_789.0:1~2"); // numbers with special chars
        expect_valid("a~1_b~2_c~3"); // repeating pattern
        expect_valid("...___---~~~:::"); // grouped special chars

        // Boundary pattern tests
        expect_valid("~a"); // tilde start
        expect_valid("a~"); // tilde end
        expect_valid("-a"); // hyphen start
        expect_valid("a-"); // hyphen end
        expect_valid("_a"); // underscore start
        expect_valid("a_"); // underscore end
        expect_valid(":a"); // colon start
        expect_valid("a:"); // colon end

        // Invalid special character tests
        expect_invalid("/"); // just slash
        expect_invalid(" "); // just space
        expect_invalid("\t"); // just tab
        expect_invalid("\n"); // just newline
        expect_invalid("\r"); // just carriage return
        expect_invalid("a b"); // space in middle
        expect_invalid("a/b"); // slash in middle
        expect_invalid("a\tb"); // tab in middle
        expect_invalid("a\nb"); // newline in middle
        expect_invalid("a\rb"); // carriage return in middle

        // Invalid character combinations
        expect_invalid("hello world"); // space
        expect_invalid("hello/world"); // forward slash
        expect_invalid("hello\\world"); // backslash
        expect_invalid("hello'world"); // single quote
        expect_invalid("hello\"world"); // double quote
        expect_invalid("hello`world"); // backtick
        expect_invalid("hello!world"); // exclamation
        expect_invalid("hello@world"); // at sign
        expect_invalid("hello#world"); // hash
        expect_invalid("hello$world"); // dollar
        expect_invalid("hello%world"); // percent
        expect_invalid("hello^world"); // caret
        expect_invalid("hello&world"); // ampersand
        expect_invalid("hello*world"); // asterisk
        expect_invalid("hello(world"); // open paren
        expect_invalid("hello)world"); // close paren
        expect_invalid("hello+world"); // plus
        expect_invalid("hello=world"); // equals
        expect_invalid("hello{world"); // open brace
        expect_invalid("hello}world"); // close brace
        expect_invalid("hello[world"); // open bracket
        expect_invalid("hello]world"); // close bracket
        expect_invalid("hello|world"); // pipe
        expect_invalid("hello\\world"); // backslash
        expect_invalid("hello<world"); // less than
        expect_invalid("hello>world"); // greater than
        expect_invalid("hello?world"); // question mark
        expect_invalid("hello,world"); // comma
        expect_invalid("hello;world"); // semicolon
        expect_invalid("hello世界"); // unicode
        expect_invalid("hello👋world"); // emoji
    }
}
