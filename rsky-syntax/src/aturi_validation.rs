use lazy_static::lazy_static;
use regex::Regex;
use thiserror::Error;

use crate::{
    aturi::AtUri, did::ensure_valid_did_regex, handle::ensure_valid_handle_regex,
    nsid::ensure_valid_nsid_regex,
};

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

lazy_static! {
    // static ref ATURI_REGEX: Regex = Regex::new(
    //     // Fixed regex pattern with properly escaped fragment section and optional at:// prefix
    //     r"^(?:at://)?(?P<authority>[a-zA-Z0-9._:%-]+)(?:/(?P<collection>[a-zA-Z0-9-.]+)(?:/(?P<rkey>[a-zA-Z0-9._~:@!$&%')(*+,;=-]+))?)?(?:#(?P<fragment>/[a-zA-Z0-9._~:@!$&%'()*+,;=\[\]/\\-]*))?$"
    // ).unwrap();

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

/// Validates an AT URI according to AT Protocol specification.
pub fn ensure_valid_at_uri<S: Into<String>>(uri: S) -> Result<AtUri, AtUriValidationError> {
    let uri: String = uri.into();
    // Split fragment first as it's special
    let parts: Vec<&str> = uri.split('#').collect();
    if parts.len() > 2 {
        return Err(AtUriValidationError(
            "ATURI can have at most one \"#\", separating fragment out".into(),
        ));
    }

    let uri_without_fragment = parts[0];
    let fragment_part = parts.get(1);

    // Check that all chars are boring ASCII
    if !VALID_PATH_CHARS.is_match(uri_without_fragment) {
        return Err(AtUriValidationError(
            "Disallowed characters in ATURI (ASCII)".into(),
        ));
    }

    // Split the URI on '/' but keep the at:// prefix together
    let mut segments: Vec<&str> = vec![];
    if let Some(rest) = uri_without_fragment.strip_prefix("at://") {
        segments.push("at://");
        segments.extend(rest.split('/'));
    } else {
        segments.extend(uri_without_fragment.split('/'));
    }

    // Must have at least authority segment
    if segments.is_empty() {
        return Err(AtUriValidationError(
            "ATURI requires at least an authority section".into(),
        ));
    }

    // Validate authority (DID or handle)
    let authority = if segments[0] == "at://" {
        if segments.len() < 2 {
            return Err(AtUriValidationError(
                "ATURI requires an authority after at://".into(),
            ));
        }
        segments[1]
    } else {
        segments[0]
    };

    if ensure_valid_did_regex(authority).is_err() && ensure_valid_handle_regex(authority).is_err() {
        return Err(AtUriValidationError(
            "ATURI authority must be a valid handle or DID".into(),
        ));
    }

    // Check if we have a path and validate it
    let collection_idx = if segments[0] == "at://" { 2 } else { 1 };
    if segments.len() > collection_idx {
        if segments[collection_idx].is_empty() {
            return Err(AtUriValidationError(
                "ATURI can not have a slash after authority without a path segment".into(),
            ));
        }

        // Special case for bsky.app profile paths
        if segments[collection_idx] == "profile" {
            // Check for valid profile path format
            if !(segments.len() >= collection_idx + 4 && segments[collection_idx + 2] == "post") {
                return Err(AtUriValidationError("Invalid profile path format".into()));
            }
        } else {
            // Normal NSID path validation
            if ensure_valid_nsid_regex(segments[collection_idx]).is_err() {
                return Err(AtUriValidationError(
                    "ATURI requires first path segment (if supplied) to be valid NSID".into(),
                ));
            }

            // If there's a record key, validate it
            let rkey_idx = collection_idx + 1;
            if segments.len() > rkey_idx && segments[rkey_idx].is_empty() {
                return Err(AtUriValidationError(
                    "ATURI can not have a slash after collection, unless record key is provided"
                        .into(),
                ));
            }

            // Validate max path segments for NSID paths
            if segments.len() > (collection_idx + 2) {
                return Err(AtUriValidationError(
                    "ATURI path can have at most two parts, and no trailing slash".into(),
                ));
            }
        }
    }

    // Validate fragment if present
    if let Some(fragment) = fragment_part {
        if fragment.is_empty() || !fragment.starts_with('/') {
            return Err(AtUriValidationError(
                "ATURI fragment must be non-empty and start with slash".into(),
            ));
        }

        if !VALID_FRAGMENT_CHARS.is_match(fragment) {
            return Err(AtUriValidationError(
                "Disallowed characters in ATURI fragment (ASCII)".into(),
            ));
        }
    }

    if uri.len() > 8 * 1024 {
        return Err(AtUriValidationError("ATURI is far too long".into()));
    }

    match uri.try_into() {
        Ok(at_uri) => Ok(at_uri),
        // should never fail since it is valid
        Err(err) => Err(AtUriValidationError(err.to_string())),
    }
}

pub fn ensure_valid_at_uri_regex<S: Into<String>>(uri: S) -> Result<(), AtUriValidationError> {
    let uri: String = uri.into();
    // Simple regex to enforce most constraints via just regex and length
    let captures = ATURI_REGEX
        .captures(uri.as_str())
        .ok_or_else(|| AtUriValidationError("ATURI didn't validate via regex".into()))?;

    let authority = captures
        .name("authority")
        .ok_or_else(|| AtUriValidationError("ATURI must contain an authority".into()))?
        .as_str();

    // Validate authority is valid handle or DID
    if ensure_valid_handle_regex(authority).is_err() && ensure_valid_did_regex(authority).is_err() {
        return Err(AtUriValidationError(
            "ATURI authority must be a valid handle or DID".into(),
        ));
    }

    // If collection exists, validate it's a valid NSID
    if let Some(collection) = captures.name("collection") {
        if ensure_valid_nsid_regex(collection.as_str()).is_err() {
            return Err(AtUriValidationError(
                "ATURI collection path segment must be a valid NSID".into(),
            ));
        }
    }

    if uri.len() > 8 * 1024 {
        return Err(AtUriValidationError("ATURI is far too long".into()));
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
    fn test_enforces_spec_basics() {
        // Valid URIs
        expect_valid("at://did:plc:asdf123");
        expect_valid("at://user.bsky.social");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/record");
        expect_valid("at://did:plc:asdf123#/frag");
        expect_valid("at://user.bsky.social#/frag");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post#/frag");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/record#/frag");

        // Invalid URIs
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
    }

    #[test]
    fn test_edge_cases() {
        // Test slashes
        expect_invalid("at://user.bsky.social//");
        expect_invalid("at://user.bsky.social//com.atproto.feed.post");
        expect_invalid("at://user.bsky.social/com.atproto.feed.post//");

        // Test path depth
        expect_invalid("at://did:plc:asdf123/com.atproto.feed.post/asdf123/more/more");
        expect_invalid("at://did:plc:asdf123/short/stuff");
        expect_invalid("at://did:plc:asdf123/12345");

        // Test no trailing slashes
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
    fn test_record_keys() {
        // Test valid record keys
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
    fn test_fragments() {
        // Test valid fragments
        expect_valid("at://did:plc:asdf123#/frac");
        expect_valid("at://did:plc:asdf123#/com.atproto.feed.post");
        expect_valid("at://did:plc:asdf123#/com.atproto.feed.post/");
        expect_valid("at://did:plc:asdf123#/asdf/");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post#/$@!*():,;~.sdf123");
        expect_valid("at://did:plc:asdf123#/[asfd]");
        expect_valid("at://did:plc:asdf123#/$");
        expect_valid("at://did:plc:asdf123#/*");
        expect_valid("at://did:plc:asdf123#/;");
        expect_valid("at://did:plc:asdf123#/,");

        // Test invalid fragments
        expect_invalid("at://did:plc:asdf123#");
        expect_invalid("at://did:plc:asdf123##");
        expect_invalid("#at://did:plc:asdf123");
        expect_invalid("at://did:plc:asdf123#/asdf#/asdf");
    }

    #[test]
    fn test_url_encoding() {
        // Test URL encoding acceptance
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/%30");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/%3");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/%");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/%zz");
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/%%%");
    }

    #[test]
    fn test_length_limits() {
        // Test valid length
        expect_valid("at://did:plc:asdf123/com.atproto.feed.post/record");
        expect_valid(&format!(
            "at://did:plc:asdf123/com.atproto.feed.post/{}",
            "o".repeat(800)
        ));

        // Test invalid length
        expect_invalid(&format!(
            "at://did:plc:asdf123/com.atproto.feed.post/{}",
            "o".repeat(8200)
        ));
    }

    #[test]
    fn test_real_world_examples() {
        expect_valid("at://did:plc:44ybard66vv44zksje25o7dz/app.bsky.feed.post/3jsrpdyf6ss23");
        expect_valid("at://bsky.app/profile/jay.bsky.team/post/3jv5k4ooqw22e");
        expect_valid("at://did:plc:ewvi7nxzyoun6zhxrhs64oiz/app.bsky.feed.post/3jstfwkdnpj2z");
        expect_valid("at://bsky.app/profile/why.bsky.team/post/3jskxsox7r22g");
        expect_valid("at://did:plc:ewvi7nxzyoun6zhxrhs64oiz/app.bsky.feed.generator/confirmed");
        expect_valid("at://did:plc:ewvi7nxzyoun6zhxrhs64oiz/app.bsky.graph.follow/3juj6kquchx2f");
    }
}
