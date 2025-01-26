use lazy_static::lazy_static;
use regex::Regex;
use thiserror::Error;

pub const INVALID_HANDLE: &str = "handle.invalid";

lazy_static! {
    static ref DISALLOWED_TLDS: Vec<&'static str> = vec![
        ".local",
        ".arpa",
        ".invalid",
        ".localhost",
        ".internal",
        ".example",
        ".alt",
        // policy could conceivably change on ".onion" some day
        ".onion",
        // NOTE: .test is allowed in testing and development. In practical terms
        // "should" "never" actually resolve and get registered in production
    ];

    // Regex for basic ASCII character validation
    static ref ASCII_CHARS_REGEX: Regex = Regex::new(r"^[a-zA-Z0-9.-]*$").unwrap();

    // Regex for TLD letter validation
    static ref TLD_START_LETTER_REGEX: Regex = Regex::new(r"^[a-zA-Z]").unwrap();

    // Complex regex for full handle validation
    static ref HANDLE_FULL_REGEX: Regex = Regex::new(
        r"^([a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?\.)+[a-zA-Z]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?$"
    ).unwrap();
}

#[derive(Error, Debug)]
pub enum HandleError {
    #[error("HandleError: Invalid Handle {0}")]
    InvalidHandle(String),
    #[error("HandleError: Reserved Handle {0}")]
    ReservedHandle(String),
    #[error("HandleError: Unsupported Domain {0}")]
    UnsupportedDomain(String),
    #[error("HandleError: Disallowed Domain {0}")]
    DisallowedDomain(String),
}

pub fn is_valid_tld<S: Into<String>>(handle: S) -> bool {
    let handle: String = handle.into();
    let handle_lower = handle.to_lowercase();
    !DISALLOWED_TLDS
        .iter()
        .any(|domain| handle_lower.ends_with(domain))
}

// Handle constraints, in English:
//  - must be a possible domain name
//    - RFC-1035 is commonly referenced, but has been updated. eg, RFC-3696,
//      section 2. and RFC-3986, section 3. can now have leading numbers (eg,
//      4chan.org)
//    - "labels" (sub-names) are made of ASCII letters, digits, hyphens
//    - can not start or end with a hyphen
//    - TLD (last component) should not start with a digit
//    - can't end with a hyphen (can end with digit)
//    - each segment must be between 1 and 63 characters (not including any periods)
//    - overall length can't be more than 253 characters
//    - separated by (ASCII) periods; does not start or end with period
//    - case insensitive
//    - domains (handles) are equal if they are the same lower-case
//    - punycode allowed for internationalization
//  - no whitespace, null bytes, joining chars, etc
//  - does not validate whether domain or TLD exists, or is a reserved or
//    special TLD (eg, .onion or .local)
//  - does not validate punycode
pub fn ensure_valid_handle<S: Into<String>>(handle: S) -> Result<(), HandleError> {
    let handle: String = handle.into();
    // Check that all chars are boring ASCII
    if !ASCII_CHARS_REGEX.is_match(&handle) {
        return Err(HandleError::InvalidHandle(
            "Disallowed characters in handle (ASCII letters, digits, dashes, periods only)".into(),
        ));
    }

    // Check overall length
    if handle.len() > 253 {
        return Err(HandleError::InvalidHandle(
            "Handle is too long (253 chars max)".into(),
        ));
    }

    // Split into labels and validate each part
    let labels: Vec<&str> = handle.split('.').collect();
    if labels.len() < 2 {
        return Err(HandleError::InvalidHandle(
            "Handle domain needs at least two parts".into(),
        ));
    }

    for (i, label) in labels.iter().enumerate() {
        if label.is_empty() {
            return Err(HandleError::InvalidHandle(
                "Handle parts can not be empty".into(),
            ));
        }

        if label.len() > 63 {
            return Err(HandleError::InvalidHandle(
                "Handle part too long (max 63 chars)".into(),
            ));
        }

        if label.ends_with('-') || label.starts_with('-') {
            return Err(HandleError::InvalidHandle(
                "Handle parts can not start or end with hyphens".into(),
            ));
        }

        // Check if it's the last label (TLD) and validate it starts with a letter
        if i == labels.len() - 1 && !TLD_START_LETTER_REGEX.is_match(label) {
            return Err(HandleError::InvalidHandle(
                "TLD must start with ASCII letter".into(), // Changed error message
            ));
        }
    }

    Ok(())
}

pub fn ensure_valid_handle_regex<S: Into<String>>(handle: S) -> Result<(), HandleError> {
    let handle: String = handle.into();
    if !HANDLE_FULL_REGEX.is_match(&handle) {
        return Err(HandleError::InvalidHandle(
            "Handle didn't validate via regex".into(),
        ));
    }

    if handle.len() > 253 {
        return Err(HandleError::InvalidHandle(
            "Handle is too long (253 chars max)".into(),
        ));
    }

    Ok(())
}

pub fn normalize_handle<S: Into<String>>(handle: S) -> String {
    let handle: String = handle.into();
    handle.to_lowercase()
}

pub fn normalize_and_ensure_valid_handle<S: Into<String>>(
    handle: S,
) -> Result<String, HandleError> {
    let normalized = normalize_handle(handle);
    ensure_valid_handle(&normalized)?;
    Ok(normalized)
}

pub fn is_valid_handle<S: Into<String>>(handle: S) -> bool {
    ensure_valid_handle(handle).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_handles() {
        assert!(is_valid_handle("test.com"));
        assert!(is_valid_handle("sub.test.com"));
        assert!(is_valid_handle("t-e-s-t.com"));
        assert!(is_valid_handle("123test.com"));
    }

    #[test]
    fn test_invalid_handles() {
        assert!(!is_valid_handle("test")); // single label
        assert!(!is_valid_handle(".test.com")); // starts with dot
        assert!(!is_valid_handle("test.com.")); // ends with dot
        assert!(!is_valid_handle("-test.com")); // starts with hyphen
        assert!(!is_valid_handle("test-.com")); // ends with hyphen
        assert!(!is_valid_handle("te@st.com")); // invalid character
    }

    #[test]
    fn test_tld_validation() {
        assert!(is_valid_tld("test.com"));
        assert!(!is_valid_tld("test.local"));
        assert!(!is_valid_tld("test.onion"));
    }

    #[test]
    fn test_normalization() {
        assert_eq!(normalize_handle("Test.COM"), "test.com");
    }

    // Helper function to check error messages
    fn assert_handle_error(result: Result<(), HandleError>, expected_msg: &str) {
        match result {
            Ok(_) => panic!("Expected error, got Ok"),
            Err(e) => {
                assert!(
                    e.to_string().contains(expected_msg),
                    "Expected error message '{}', got '{}'",
                    expected_msg,
                    e.to_string()
                );
            }
        }
    }

    #[test]
    fn test_disallowed_tlds() {
        // Test each disallowed TLD
        let test_handle = "example";
        for tld in DISALLOWED_TLDS.iter() {
            let handle = format!("{}{}", test_handle, tld);
            assert!(
                !is_valid_tld(&handle),
                "Handle '{}' with disallowed TLD '{}' should be invalid",
                handle,
                tld
            );
        }
    }

    #[test]
    fn test_error_ascii_chars() {
        let result = ensure_valid_handle("test!.com");
        assert_handle_error(result, "Disallowed characters");

        let result = ensure_valid_handle("täst.com");
        assert_handle_error(result, "Disallowed characters");

        let result = ensure_valid_handle("test$.com");
        assert_handle_error(result, "Disallowed characters");
    }

    #[test]
    fn test_error_handle_length() {
        // Generate a handle that's too long (254 chars)
        let long_prefix = "a".repeat(250);
        let long_handle = format!("{}.com", long_prefix);
        let result = ensure_valid_handle(&long_handle);
        assert_handle_error(result, "Handle is too long");
    }

    #[test]
    fn test_error_handle_parts() {
        // Test single part
        let result = ensure_valid_handle("testonly");
        assert_handle_error(result, "needs at least two parts");

        // Test empty parts
        let result = ensure_valid_handle("test..com");
        assert_handle_error(result, "parts can not be empty");

        let result = ensure_valid_handle(".test.com");
        assert_handle_error(result, "parts can not be empty");

        let result = ensure_valid_handle("test.com.");
        assert_handle_error(result, "parts can not be empty");
    }

    #[test]
    fn test_error_part_length() {
        // Generate a label that's too long (64 chars)
        let long_label = "a".repeat(64);
        let long_handle = format!("{}.com", long_label);
        let result = ensure_valid_handle(&long_handle);
        assert_handle_error(result, "part too long");
    }

    #[test]
    fn test_error_hyphen_usage() {
        // Test start hyphen
        let result = ensure_valid_handle("-test.com");
        assert_handle_error(result, "can not start or end with hyphens");

        // Test end hyphen
        let result = ensure_valid_handle("test-.com");
        assert_handle_error(result, "can not start or end with hyphens");

        // Test both start and end hyphens
        let result = ensure_valid_handle("-test-.com");
        assert_handle_error(result, "can not start or end with hyphens");

        // Test hyphen in TLD
        let result = ensure_valid_handle("test.-com");
        assert_handle_error(result, "can not start or end with hyphens");
    }

    #[test]
    fn test_error_tld_start_letter() {
        // Test TLD starting with number
        let result = ensure_valid_handle("test.1com");
        assert_handle_error(result, "TLD must start with ASCII letter");

        // Test TLD starting with hyphen
        let result = ensure_valid_handle("test.-com");
        assert_handle_error(result, "can not start or end with hyphens");
    }

    #[test]
    fn test_regex_validation() {
        // Test regex specific validation
        let result = ensure_valid_handle_regex("test!.com");
        assert_handle_error(result, "Handle didn't validate via regex");

        let result = ensure_valid_handle_regex("-test.com");
        assert_handle_error(result, "Handle didn't validate via regex");
    }

    #[test]
    fn test_normalize_and_ensure_valid() {
        // Test successful normalization
        assert_eq!(
            normalize_and_ensure_valid_handle("Test.COM").unwrap(),
            "test.com"
        );

        // Test normalization with invalid handle
        let result = normalize_and_ensure_valid_handle("Test$.COM");
        assert!(result.is_err());
    }

    #[test]
    fn test_handle_edge_cases() {
        // Test handle with multiple dots
        let result = ensure_valid_handle("test.sub.domain.com");
        assert!(result.is_ok());

        // Test handle with allowed hyphens
        let result = ensure_valid_handle("test-sub.test-domain.com");
        assert!(result.is_ok());

        // Test handle with numbers
        let result = ensure_valid_handle("test123.com");
        assert!(result.is_ok());

        // Test minimum length handle
        let result = ensure_valid_handle("a.co");
        assert!(result.is_ok());
    }

    #[test]
    fn test_tld_edge_cases() {
        // Test similar but allowed TLDs
        assert!(is_valid_tld("test.commons"));
        assert!(is_valid_tld("test.localdata"));
        assert!(is_valid_tld("test.examples"));

        // Test TLD case sensitivity
        assert!(!is_valid_tld("test.LOCAL"));
        assert!(!is_valid_tld("test.ARPA"));

        // Test TLD with preceding dot
        assert!(!is_valid_tld("test..local"));
        assert!(!is_valid_tld("test..arpa"));
    }

    #[test]
    fn test_various_invalid_combinations() {
        let invalid_cases = vec![
            ("test.com.", "parts can not be empty"),
            ("test", "needs at least two parts"),
            ("test.123", "TLD must start with ASCII letter"),
            ("test.com-", "can not start or end with hyphens"),
            ("test.-com", "can not start or end with hyphens"),
            ("t@st.com", "Disallowed characters"),
            ("test.c*m", "Disallowed characters"),
            ("", "needs at least two parts"),
            (" .com", "Disallowed characters"),
            ("test. com", "Disallowed characters"),
        ];

        for (invalid_handle, expected_error) in invalid_cases {
            let result = ensure_valid_handle(invalid_handle);
            assert_handle_error(result, expected_error);
        }
    }
}
