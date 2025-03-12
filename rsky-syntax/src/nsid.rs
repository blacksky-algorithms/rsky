use lazy_static::lazy_static;
use regex::Regex;
use std::fmt::Display;
use thiserror::Error;

/*
Grammar:

alpha     = "a" / "b" / "c" / "d" / "e" / "f" / "g" / "h" / "i" / "j" / "k" / "l" / "m" / "n" /
            "o" / "p" / "q" / "r" / "s" / "t" / "u" / "v" / "w" / "x" / "y" / "z" / "A" / "B" /
            "C" / "D" / "E" / "F" / "G" / "H" / "I" / "J" / "K" / "L" / "M" / "N" / "O" / "P" /
            "Q" / "R" / "S" / "T" / "U" / "V" / "W" / "X" / "Y" / "Z"
number    = "1" / "2" / "3" / "4" / "5" / "6" / "7" / "8" / "9" / "0"
delim     = "."
segment   = alpha *( alpha / number / "-" )
authority = segment *( delim segment )
name      = alpha *( alpha / number )
nsid      = authority delim name
*/

lazy_static! {
    // Regex for basic ASCII character validation
    static ref ASCII_CHARS_REGEX: Regex = Regex::new(r"^[a-zA-Z0-9.-]*$").unwrap();

    // Complex regex for full NSID validation
    static ref NSID_FULL_REGEX: Regex = Regex::new(
        r"^[a-zA-Z]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(\.[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)+(\.[a-zA-Z][a-zA-Z0-9]{0,62})$"
    ).unwrap();
}

#[derive(Error, Debug)]
#[error("InvalidNsidError: {0}")]
pub struct InvalidNsidError(String);

// Human readable constraints on NSID:
// - a valid domain in reversed notation
// - followed by an additional period-separated name, which is alphanumeric and starts with a letter
pub fn ensure_valid_nsid<S: Into<String>>(nsid: S) -> Result<(), InvalidNsidError> {
    let nsid: String = nsid.into();
    // Check that all chars are boring ASCII
    if !ASCII_CHARS_REGEX.is_match(&nsid) {
        return Err(InvalidNsidError(
            "Disallowed characters in NSID (ASCII letters, digits, dashes, periods only)".into(),
        ));
    }

    // Check overall length
    if nsid.len() > 253 + 1 + 63 {
        return Err(InvalidNsidError("NSID is too long (317 chars max)".into()));
    }

    // Split into labels and validate each part
    let labels: Vec<&str> = nsid.split('.').collect();
    if labels.len() < 3 {
        return Err(InvalidNsidError("NSID needs at least three parts".into()));
    }

    for (i, label) in labels.iter().enumerate() {
        if label.is_empty() {
            return Err(InvalidNsidError("NSID parts can not be empty".into()));
        }

        if label.len() > 63 {
            return Err(InvalidNsidError("NSID part too long (max 63 chars)".into()));
        }

        let is_last_segment = i == labels.len() - 1;

        // Apply rules for domain authority segments (all but the last segment)
        if !is_last_segment {
            if label.ends_with('-') || label.starts_with('-') {
                return Err(InvalidNsidError(
                    "NSID authority parts can not start or end with hyphen".into(),
                ));
            }

            if i == 0 && label.starts_with(char::is_numeric) {
                return Err(InvalidNsidError(
                    "NSID first part may not start with a digit".into(),
                ));
            }
        } else {
            // Validate the final name segment according to updated spec

            // Check if the name starts with a letter
            if !label.starts_with(|c: char| c.is_ascii_alphabetic()) {
                return Err(InvalidNsidError(
                    "NSID name must start with a letter".into(),
                ));
            }

            // Check if the name contains only alphanumeric characters (no hyphens)
            if !label.chars().all(|c| c.is_ascii_alphanumeric()) {
                return Err(InvalidNsidError(
                    "NSID name must only contain letters and digits (no hyphens)".into(),
                ));
            }
        }
    }

    Ok(())
}

pub fn ensure_valid_nsid_regex(nsid: &str) -> Result<(), InvalidNsidError> {
    if !NSID_FULL_REGEX.is_match(nsid) {
        return Err(InvalidNsidError("NSID didn't validate via regex".into()));
    }

    if nsid.len() > 253 + 1 + 63 {
        return Err(InvalidNsidError("NSID is too long (317 chars max)".into()));
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Nsid {
    segments: Vec<String>,
}

impl Nsid {
    pub fn parse<S: Into<String>>(nsid: S) -> Result<Self, InvalidNsidError> {
        let nsid: String = nsid.into();
        ensure_valid_nsid(&nsid)?;
        Ok(Self {
            segments: nsid.split('.').map(String::from).collect(),
        })
    }

    pub fn create(authority: &str, name: &str) -> Result<Self, InvalidNsidError> {
        let mut segments: Vec<String> = authority.split('.').rev().map(String::from).collect();
        segments.push(name.to_string());
        let nsid = segments.join(".");
        ensure_valid_nsid(&nsid)?;
        Ok(Self { segments })
    }

    pub fn authority(&self) -> String {
        self.segments[..self.segments.len() - 1]
            .iter()
            .rev()
            .cloned()
            .collect::<Vec<String>>()
            .join(".")
    }

    pub fn name(&self) -> &str {
        self.segments.last().unwrap()
    }
}

impl Display for Nsid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.segments.join("."))
    }
}

impl TryFrom<&str> for Nsid {
    type Error = InvalidNsidError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Nsid::parse(value.to_string())
    }
}

impl TryFrom<String> for Nsid {
    type Error = InvalidNsidError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Nsid::parse(value.to_string())
    }
}

impl TryFrom<&String> for Nsid {
    type Error = InvalidNsidError;

    fn try_from(value: &String) -> Result<Self, Self::Error> {
        Nsid::parse(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expect_valid(nsid: &str) {
        ensure_valid_nsid(nsid).unwrap();
        ensure_valid_nsid_regex(nsid).unwrap();
    }

    fn expect_invalid(nsid: &str) {
        assert!(ensure_valid_nsid(nsid).is_err());
        assert!(ensure_valid_nsid_regex(nsid).is_err());
    }

    #[test]
    fn test_nsid_parsing_and_creation() {
        let nsid = Nsid::parse("com.example.foo").unwrap();
        assert_eq!(nsid.authority(), "example.com");
        assert_eq!(nsid.name(), "foo");
        assert_eq!(nsid.to_string(), "com.example.foo");

        let nsid = Nsid::parse("com.long-thing1.cool.fooBar123").unwrap();
        assert_eq!(nsid.authority(), "cool.long-thing1.com");
        assert_eq!(nsid.name(), "fooBar123");
        assert_eq!(nsid.to_string(), "com.long-thing1.cool.fooBar123");
    }

    #[test]
    fn test_nsid_creation() {
        let nsid = Nsid::create("example.com", "foo").unwrap();
        assert_eq!(nsid.authority(), "example.com");
        assert_eq!(nsid.name(), "foo");
        assert_eq!(nsid.to_string(), "com.example.foo");

        let nsid = Nsid::create("cool.long-thing1.com", "fooBar123").unwrap();
        assert_eq!(nsid.authority(), "cool.long-thing1.com");
        assert_eq!(nsid.name(), "fooBar123");
        assert_eq!(nsid.to_string(), "com.long-thing1.cool.fooBar123");
    }

    #[test]
    fn test_enforces_spec_details() {
        // Valid NSIDs
        expect_valid("com.example.foo");
        expect_valid("net.users.bob.ping");
        expect_valid("a.b.c");
        expect_valid("m.xn--masekowski-d0b.pl");
        expect_valid("one.two.three");
        expect_valid("one.two.three.four-and.FiVe");
        expect_valid("one.2.three");
        expect_valid("a-0.b-1.c");
        expect_valid("a0.b1.cc");
        expect_valid("cn.8.lex.stuff");
        expect_valid("test.12345.record");
        expect_valid("a01.thing.record");
        expect_valid("a.0.c");
        expect_valid("xn--fiqs8s.xn--fiqa61au8b7zsevnm8ak20mc4a87e.record.two");
        expect_valid("a0.b1.c3");
        expect_valid("com.example.f00");

        // Test max length segments
        let long_nsid = format!("com.{}.foo", "o".repeat(63));
        expect_valid(&long_nsid);

        // Invalid NSIDs
        expect_invalid("com.example.foo.*");
        expect_invalid("com.example.foo.blah*");
        expect_invalid("com.example.foo.*blah");
        expect_invalid("com.exa💩ple.thing");
        expect_invalid("a-0.b-1.c-");
        expect_invalid("example.com");
        expect_invalid("com.example");
        expect_invalid("a.");
        expect_invalid(".one.two.three");
        expect_invalid("one.two.three ");
        expect_invalid("one.two..three");
        expect_invalid("one .two.three");
        expect_invalid(" one.two.three");
        expect_invalid("com.exa💩ple.thing");
        expect_invalid("com.atproto.feed.p@st");
        expect_invalid("com.atproto.feed.p_st");
        expect_invalid("com.atproto.feed.p*st");
        expect_invalid("com.atproto.feed.po#t");
        expect_invalid("com.atproto.feed.p!ot");
        expect_invalid("com.example-.foo");
        expect_invalid("com.example.3"); // Name starts with digit
        expect_invalid("com.example.foo-bar"); // Name with hyphen

        // Test segments too long
        let too_long_nsid = format!("com.{}.foo", "o".repeat(64));
        expect_invalid(&too_long_nsid);

        // Test overall too long
        let too_long_overall = format!("com.{}.foo", "middle.".repeat(50));
        expect_invalid(&too_long_overall);
    }

    #[test]
    fn test_allows_onion_nsids() {
        expect_valid("onion.expyuzz4wqqyqhjn.spec.getThing");
        expect_valid(
            "onion.g2zyxa5ihm7nsggfxnu52rck2vv4rvmdlkiu3zzui5du4xyclen53wid.lex.deleteThing",
        );
    }

    #[test]
    fn test_allows_numeric_segments() {
        expect_valid("org.4chan.lex.getThing");
        expect_valid("cn.8.lex.stuff");
        expect_valid(
            "onion.2gzyxa5ihm7nsggfxnu52rck2vv4rvmdlkiu3zzui5du4xyclen53wid.lex.deleteThing",
        );
    }
}
