use lazy_static::lazy_static;
use regex::Regex;
use thiserror::Error;

use crate::{
    aturi::AtUri,
    did::{ensure_valid_did, ensure_valid_did_regex},
    handle::{ensure_valid_handle, ensure_valid_handle_regex},
    nsid::{ensure_valid_nsid, ensure_valid_nsid_regex},
};

// Note: Typescript implementation allows for (8 * 1024) bytes
const MAX_URI_LEN: usize = 512;

lazy_static! {
    static ref ATURI_REGEX: Regex = Regex::new(
        // Support both NSID-style paths and bsky profile paths
        r"^at://(?P<authority>[a-zA-Z0-9._:%-]+)(?:/(?:(?P<collection>[a-zA-Z0-9-.]+)(?:/(?P<rkey>[a-zA-Z0-9._~:@!$&%')(*+,;=-]+))?|profile/[a-zA-Z0-9.-]+/post/[a-zA-Z0-9]+))?(?:#(?P<fragment>/[a-zA-Z0-9._~:@!$&%'()*+,;=\[\]/\\-]*))?$"
    ).unwrap();

    // Path component characters
    static ref VALID_PATH_CHARS: Regex = Regex::new(r"^[a-zA-Z0-9._~:@!$&'()*+,;=%/-]*$").unwrap();

    // Fragment validation - fixed pattern
    static ref VALID_FRAGMENT_CHARS: Regex = Regex::new(r"^/[a-zA-Z0-9._~:@!$&'()*+,;=\[\]/\\-]*$").unwrap();
}

#[derive(Error, Debug)]
#[error("AtUriValidationError: {0}")]
pub struct AtUriValidationError(String);

// Human-readable constraints on ATURI:
//   - following regular URLs, a 8KByte hard total length limit
//   - follows ATURI docs on website
//      - all ASCII characters, no whitespace. non-ASCII could be URL-encoded
//      - starts "at://"
//      - "authority" is a valid DID or a valid handle
//      - optionally, follow "authority" with "/" and valid NSID as start of path
//      - optionally, if NSID given, follow that with "/" and rkey
//      - rkey path component can include URL-encoded ("percent encoded"), or:
//          ALPHA / DIGIT / "-" / "." / "_" / "~" / ":" / "@" / "!" / "$" / "&" / "'" / "(" / ")" / "*" / "+" / "," / ";" / "="
//          [a-zA-Z0-9._~:@!$&\'()*+,;=-]
//      - rkey must have at least one char
//      - regardless of path component, a fragment can follow as "#" and then a JSON pointer (RFC-6901)
pub fn ensure_valid_at_uri<S: Into<String>>(uri: S) -> Result<AtUri, AtUriValidationError> {
    let uri: String = uri.into();
    let uri_parts = uri.split("#").map(|p| p.into()).collect::<Vec<String>>();
    if uri_parts.len() > 2 {
        return Err(AtUriValidationError(
            "ATURI can have at most one '#', separating fragment out".into(),
        ));
    }
    let hacky_empty = String::from("");
    let fragment_part = uri_parts.get(1).unwrap_or_else(|| &hacky_empty).to_string();
    let uri = uri_parts.first().unwrap().to_string();
    // check that all chars are boring ASCII
    if !VALID_PATH_CHARS.is_match(&uri) {
        return Err(AtUriValidationError(
            "Disallowed characters in ATURI (ASCII)".into(),
        ));
    }

    let at_colon = String::from("at:");
    let parts = uri.split("/").map(|p| p.into()).collect::<Vec<String>>();
    if parts.len() >= 3 && (parts[0] != at_colon || !parts[1].is_empty()) {
        return Err(AtUriValidationError(
            "ATURI must start with \"at://\"".into(),
        ));
    }
    if parts.len() < 3 {
        return Err(AtUriValidationError(
            "ATURI requires at least method and authority sections".into(),
        ));
    }

    use std::result::Result::Ok as Okay;
    {
        match (
            ensure_valid_did(parts[2].clone()),
            ensure_valid_handle(parts[2].clone()),
        ) {
            (Err(_), Err(_)) => Err(AtUriValidationError(
                "ATURI authority must be a valid handle or DID".into(),
            )),
            (Okay(()), _) => {
                if !parts[2].starts_with("did:") {
                    return Err(AtUriValidationError(
                        "ATURI authority is not in a valid DID format".into(),
                    ));
                }
                Okay(())
            }
            (_, Okay(())) => Okay(()),
        }
    }?;

    if parts.len() >= 4 {
        if parts[3].is_empty() {
            return Err(AtUriValidationError(
                "ATURI can not have a slash after authority without a path segment".into(),
            ));
        }
        if let Err(e) = ensure_valid_nsid(parts[3].clone()) {
            return Err(AtUriValidationError(e.to_string()));
        }
    }

    if parts.len() >= 5 && parts[4].is_empty() {
        return Err(AtUriValidationError(
            "ATURI can not have a slash after collection, unless record key is provided".into(),
        ));
    }

    if parts.len() >= 6 {
        return Err(AtUriValidationError(
            "ATURI path can have at most two parts, and no trailing slash".into(),
        ));
    }

    if uri_parts.len() >= 2 && fragment_part.is_empty() {
        return Err(AtUriValidationError(
            "ATURI fragment must be non-empty and start with slash".into(),
        ));
    }

    if !fragment_part.is_empty() {
        if fragment_part.is_empty() || !fragment_part.starts_with("/") {
            return Err(AtUriValidationError(
                "ATURI fragment must be non-empty and start with slash".into(),
            ));
        }
        // NOTE: enforcing *some* checks here for sanity. Eg, at least no whitespace
        if !VALID_FRAGMENT_CHARS.is_match(&fragment_part) {
            return Err(AtUriValidationError(
                "Disallowed characters in ATURI fragment (ASCII)".into(),
            ));
        }
    }

    if uri.len() > MAX_URI_LEN {
        return Err(AtUriValidationError("ATURI is far too long".into()));
    }
    match uri.try_into() {
        Okay(at_uri) => Okay(at_uri),
        // should never fail since it is valid
        Err(err) => Err(AtUriValidationError(err.to_string())),
    }
}

pub fn ensure_valid_at_uri_regex<S: Into<String>>(uri: S) -> Result<(), AtUriValidationError> {
    let uri: String = uri.into();
    let captures = ATURI_REGEX
        .captures(&uri)
        .ok_or_else(|| AtUriValidationError("ATURI didn't validate via regex".to_string()))?;

    if let Some(authority) = captures.name("authority") {
        use std::result::Result::Ok as Okay;
        {
            match (
                ensure_valid_did_regex(authority.as_str()),
                ensure_valid_handle_regex(authority.as_str()),
            ) {
                (Err(_), Err(_)) => Err(AtUriValidationError(
                    "ATURI authority must be a valid handle or DID".into(),
                )),
                (Okay(()), _) => Okay(()),
                (_, Okay(())) => Okay(()),
            }
        }?;
    }

    if let Some(collection) = captures.name("collection") {
        if let Err(e) = ensure_valid_nsid_regex(collection.as_str()) {
            return Err(AtUriValidationError(e.to_string()));
        }
    }

    if uri.len() > MAX_URI_LEN {
        return Err(AtUriValidationError("ATURI is far too long".to_string()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expect_valid(uri: &str) {
        ensure_valid_at_uri(uri).unwrap_or_else(|e| {
            panic!(
                "ensure_valid_at_uri validation test: Expected '{}' to be valid, but got error: {}",
                uri, e
            )
        });
        ensure_valid_at_uri_regex(uri).unwrap_or_else(|e| panic!("ensure_valid_at_uri_regex validation test: Expected '{}' to be valid, but got error: {}", uri, e));
    }

    fn expect_invalid(uri: &str) {
        assert!(ensure_valid_at_uri(uri).is_err(), "ensure_valid_at_uri invalidation test: Expected '{}' to be invalid, but it was considered valid", uri);
        assert!(ensure_valid_at_uri_regex(uri).is_err(),  "ensure_valid_at_uri_regex invalidation test: Expected '{}' to be invalid, but it was considered valid", uri);
    }

    #[test]
    fn test_debug_me() {
        expect_invalid(&format!(
            "at://did:plc:asdf123/com.atproto.feed.post/{}",
            "o".repeat(800)
        ));
    }

    #[test]
    fn test_debug_me_2() {
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post");
    }

    #[test]
    fn test_invalid_debug() {
        // Test invalid fragments
        expect_invalid("at://did:plc:asdf123#");
        expect_invalid("at://did:plc:asdf123##");
        expect_invalid("#at://did:plc:asdf123");
        expect_invalid("at://did:plc:asdf123#/asdf#/asdf");
    }

    #[test]
    fn test_aturi_syntax_valid_txt_file() {
        // enforces spec basics
        expect_valid("at://did:plc:asdf123");
        expect_valid("at://user.bsky.social");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/record");

        // enforces no trailing slashes
        expect_valid("at://did:plc:asdf123");
        expect_valid("at://user.bsky.social");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/record");

        // enforces strict paths
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/asdf123");

        // is very permissive about record keys
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/asdf123");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/a");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/asdf-123");
        expect_valid("at://did:abc:123");
        expect_valid("at://did:abc:123/io.nsid.someFunc/record-key");

        expect_valid("at://did:abc:123/io.nsid.someFunc/self.");
        expect_valid("at://did:abc:123/io.nsid.someFunc/lang:");
        expect_valid("at://did:abc:123/io.nsid.someFunc/:");
        expect_valid("at://did:abc:123/io.nsid.someFunc/-");
        expect_valid("at://did:abc:123/io.nsid.someFunc/_");
        expect_valid("at://did:abc:123/io.nsid.someFunc/~");
        expect_valid("at://did:abc:123/io.nsid.someFunc/...");
    }

    #[test]
    fn test_valid_spec_basics() {
        expect_valid("at://did:plc:asdf123");
        expect_valid("at://user.bsky.social");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/record");

        expect_valid("at://did:plc:asdf123#/frag");
        expect_valid("at://user.bsky.social#/frag");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post#/frag");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/record#/frag");
    }
    #[test]
    fn test_invalid_spec_basics() {
        expect_invalid("a://did:plc:asdf123");
        expect_invalid("at//did:plc:asdf123");
        expect_invalid("at:/a/did:plc:asdf123");
        expect_invalid("at:/did:plc:asdf123");
        expect_invalid("AT://did:plc:asdf123");
        expect_invalid("http://did:plc:asdf123");
        expect_invalid("://did:plc:asdf123");
        expect_invalid("at:did:plc:asdf123");
        expect_invalid("at:/did:plc:asdf123");
        expect_invalid("at:///did:plc:asdf123");
        expect_invalid("at://:/did:plc:asdf123");
        expect_invalid("at:/ /did:plc:asdf123");
        expect_invalid("at://did:plc:asdf123 ");
        expect_invalid("at://did:plc:asdf123/ ");
        expect_invalid(" at://did:plc:asdf123");
        expect_invalid("at://did:plc:asdf123/com.atproto.feed.post ");
        expect_invalid("at://did:plc:asdf123/com.atproto.feed.post# ");
        expect_invalid("at://did:plc:asdf123/com.atproto.feed.post#/ ");
        expect_invalid("at://did:plc:asdf123/com.atproto.feed.post#/frag ");
        expect_invalid("at://did:plc:asdf123/com.atproto.feed.post#fr ag");
        expect_invalid("//did:plc:asdf123");
        expect_invalid("at://name");
        expect_invalid("at://name.0");
        expect_invalid("at://diD:plc:asdf123");
        expect_invalid("at://did:plc:asdf123/com.atproto.feed.p@st");
        expect_invalid("at://did:plc:asdf123/com.atproto.feed.p$st");
        expect_invalid("at://did:plc:asdf123/com.atproto.feed.p%st");
        expect_invalid("at://did:plc:asdf123/com.atproto.feed.p&st");
        expect_invalid("at://did:plc:asdf123/com.atproto.feed.p()t");
        expect_invalid("at://did:plc:asdf123/com.atproto.feed_post");
        expect_invalid("at://did:plc:asdf123/-com.atproto.feed.post");
        expect_invalid("at://did:plc:asdf@123/com.atproto.feed.post");

        expect_invalid("at://DID:plc:asdf123");
        expect_invalid("at://user.bsky.123");
        expect_invalid("at://bsky");
        expect_invalid("at://did:plc:");
        expect_invalid("at://did:plc:");
        expect_invalid("at://frag");

        expect_invalid(&format!(
            "at://did:plc:asdf123/com.atproto.feed.post/{}",
            "o".repeat(8200)
        ));
    }

    #[test]
    fn test_invalid_edge_cases() {
        expect_invalid("at://user.bsky.social//");
        expect_invalid("at://user.bsky.social//com.atproto.feed.post");
        expect_invalid("at://user.bsky.social/com.atproto.feed.post//");
        expect_invalid("at://did:plc:asdf123/com.atproto.feed.post/asdf123/more/more");
        expect_invalid("at://did:plc:asdf123/short/stuff");
        expect_invalid("at://did:plc:asdf123/12345");
    }

    #[test]
    fn test_no_trailing_slashes() {
        expect_valid("at://did:plc:asdf123");
        expect_invalid("at://did:plc:asdf123/");

        expect_valid("at://user.bsky.social");
        expect_invalid("at://user.bsky.social/");

        expect_valid("at://did:plc:asdf123/com.atproto.feed.post");
        expect_invalid("at://did:plc:asdf123/com.atproto.feed.post/");

        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/record");
        expect_invalid("at://did:plc:asdf123/com.atproto.feed.post/record/");
        expect_invalid("at://did:plc:asdf123/com.atproto.feed.post/record/#/frag");
    }

    #[test]
    fn test_strict_path() {
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/asdf123");
        expect_invalid("at://did:plc:asdf123/com.atproto.feed.post/asdf123/asdf");
    }

    #[test]
    fn test_record_keys_are_very_permissive() {
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/asdf123");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/a");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/%23");

        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/$@!*)(:,;~.sdf123");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/~'sdf123");

        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/$");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/@");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/!");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/*");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/(");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/,");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/;");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/abc%30123");
    }
    #[test]
    fn test_is_provabably_too_permissive_about_url_encoding() {
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/%30");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/%3");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/%");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/%zz");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/%%%");
    }

    #[test]
    fn test_is_very_permissive_about_fragments() {
        expect_valid("at://did:plc:asdf123#/frac");

        expect_invalid("at://did:plc:asdf123#");
        expect_invalid("at://did:plc:asdf123##");
        expect_invalid("#at://did:plc:asdf123");
        expect_invalid("at://did:plc:asdf123#/asdf#/asdf");

        expect_valid("at://did:plc:asdf123#/com.atproto.feed.post");
        expect_valid("at://did:plc:asdf123#/com.atproto.feed.post/");
        expect_valid("at://did:plc:asdf123#/asdf/");

        expect_valid("at://did:plc:asdf123/com.atproto.feed.post#/$@!*():,;~.sdf123");
        expect_valid("at://did:plc:asdf123#/[asfd]");

        expect_valid("at://did:plc:asdf123#/$");
        expect_valid("at://did:plc:asdf123#/*");
        expect_valid("at://did:plc:asdf123#/;");
        expect_valid("at://did:plc:asdf123#/,");
    }
}
