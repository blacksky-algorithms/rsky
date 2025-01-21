use lazy_static::lazy_static;
use regex::Regex;
use thiserror::Error;

// Human-readable constraints:
//   - valid W3C DID (https://www.w3.org/TR/did-core/#did-syntax)
//      - entire URI is ASCII: [a-zA-Z0-9._:%-]
//      - always starts "did:" (lower-case)
//      - method name is one or more lower-case letters, followed by ":"
//      - remaining identifier can have any of the above chars, but can not end in ":"
//      - it seems that a bunch of ":" can be included, and don't need spaces between
//      - "%" is used only for "percent encoding" and must be followed by two hex characters (and thus can't end in "%")
//      - query ("?") and fragment ("#") stuff is defined for "DID URIs", but not as part of identifier itself
//      - "The current specification does not take a position on the maximum length of a DID"
//   - in current atproto, only allowing did:plc and did:web. But not *forcing* this at lexicon layer
//   - hard length limit of 8KBytes
//   - not going to validate "percent encoding" here

lazy_static! {
    // Regex for basic ASCII character validation
    static ref ASCII_CHARS_REGEX: Regex = Regex::new(r"^[a-zA-Z0-9._:%-]*$").unwrap();

    // Complex regex for full DID validation
    static ref DID_FULL_REGEX: Regex = Regex::new(r"^did:[a-z]+:[a-zA-Z0-9._:%-]*[a-zA-Z0-9._-]$").unwrap();
}

#[derive(Error, Debug)]
#[error("InvalidDidError: {0}")]
pub struct InvalidDidError(String);

pub fn ensure_valid_did<S: Into<String>>(did: S) -> Result<(), InvalidDidError> {
    let did: String = did.into();
    if !did.starts_with("did:") {
        return Err(InvalidDidError("DID requires \"did:\" prefix".into()));
    }

    // Check that all chars are boring ASCII
    if !ASCII_CHARS_REGEX.is_match(did.as_str()) {
        return Err(InvalidDidError(
            "Disallowed characters in DID (ASCII letters, digits, and a couple other characters only)".into(),
        ));
    }

    let parts: Vec<&str> = did.split(':').collect();
    if parts.len() < 3 {
        return Err(InvalidDidError(
            "DID requires prefix, method, and method-specific content".into(),
        ));
    }

    let method = parts[1];
    if !method.chars().all(|c| c.is_ascii_lowercase()) {
        return Err(InvalidDidError(
            "DID method must be lower-case letters".into(),
        ));
    }

    if did.ends_with(':') || did.ends_with('%') {
        return Err(InvalidDidError(
            "DID can not end with \":\" or \"%\"".into(),
        ));
    }

    if did.len() > 2 * 1024 {
        return Err(InvalidDidError("DID is too long (2048 chars max)".into()));
    }

    Ok(())
}

pub fn ensure_valid_did_regex<S: Into<String>>(did: S) -> Result<(), InvalidDidError> {
    let did: String = did.into();
    if !DID_FULL_REGEX.is_match(did.as_str()) {
        return Err(InvalidDidError("DID didn't validate via regex".into()));
    }

    if did.len() > 2 * 1024 {
        return Err(InvalidDidError("DID is too long (2048 chars max)".into()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expect_valid(did: &str) {
        ensure_valid_did(did).unwrap();
        ensure_valid_did_regex(did).unwrap();
    }

    fn expect_invalid(did: &str) {
        assert!(ensure_valid_did(did).is_err());
        assert!(ensure_valid_did_regex(did).is_err());
    }

    #[test]
    fn test_enforces_spec_details() {
        // Valid DIDs
        expect_valid("did:method:val");
        expect_valid("did:method:VAL");
        expect_valid("did:method:val123");
        expect_valid("did:method:123");
        expect_valid("did:method:val-two");
        expect_valid("did:method:val_two");
        expect_valid("did:method:val.two");
        expect_valid("did:method:val:two");
        expect_valid("did:method:val%BB");

        // Invalid DIDs
        expect_invalid("did");
        expect_invalid("didmethodval");
        expect_invalid("method:did:val");
        expect_invalid("did:method:");
        expect_invalid("didmethod:val");
        expect_invalid("did:methodval");
        expect_invalid(":did:method:val");
        expect_invalid("did.method.val");
        expect_invalid("did:method:val:");
        expect_invalid("did:method:val%");
        expect_invalid("DID:method:val");
        expect_invalid("did:METHOD:val");
        expect_invalid("did:m123:val");

        // Length checks
        expect_valid(&format!("did:method:{}", "v".repeat(240)));
        expect_invalid(&format!("did:method:{}", "v".repeat(8500)));
    }

    #[test]
    fn test_edge_cases() {
        expect_valid("did:m:v");
        expect_valid("did:method::::val");
        expect_valid("did:method:-");
        expect_valid("did:method:-:_:.:%ab");
        expect_valid("did:method:.");
        expect_valid("did:method:_");
        expect_valid("did:method::.");

        expect_invalid("did:method:val/two");
        expect_invalid("did:method:val?two");
        expect_invalid("did:method:val#two");
        expect_invalid("did:method:val%");
    }

    #[test]
    fn test_real_dids() {
        expect_valid("did:example:123456789abcdefghi");
        expect_valid("did:plc:7iza6de2dwap2sbkpav7c6c6");
        expect_valid("did:web:example.com");
        expect_valid("did:web:localhost%3A1234");
        expect_valid("did:key:zQ3shZc2QzApp2oymGvQbzP8eKheVshBHbU4ZYjeXqwSKEn6N");
        expect_valid("did:ethr:0xb9c5714089478a327f09197987f16f9e5d936e8a");
        expect_valid("did:onion:2gzyxa5ihm7nsggfxnu52rck2vv4rvmdlkiu3zzui5du4xyclen53wid");
    }
}
