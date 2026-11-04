//! Table-driven tests over the vendored upstream atproto interop syntax corpus
//! (`tests/interop/syntax/`). The three `language_*.txt` vector files are
//! intentionally not wired up: rsky has no BCP-47 implementation yet.

use rsky_syntax::aturi_validation::{ensure_valid_at_uri, ensure_valid_at_uri_regex};
use rsky_syntax::datetime::{ensure_valid_datetime, is_valid_datetime};
use rsky_syntax::did::{ensure_valid_did, ensure_valid_did_regex};
use rsky_syntax::handle::{ensure_valid_handle, ensure_valid_handle_regex, is_valid_handle};
use rsky_syntax::nsid::{ensure_valid_nsid, ensure_valid_nsid_regex};
use rsky_syntax::record_key::{ensure_valid_record_key, is_valid_record_key};
use rsky_syntax::tid::{ensure_valid_tid, is_valid_tid};

type Validator<'a> = (&'a str, &'a dyn Fn(&str) -> bool);

fn load_vectors(file: &str) -> Vec<String> {
    let path = format!(
        "{}/tests/interop/syntax/{}",
        env!("CARGO_MANIFEST_DIR"),
        file
    );
    let contents = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read vector file {path}: {e}"));
    contents
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.to_string())
        .collect()
}

fn check_vectors(file: &str, expected_count: usize, expect_valid: bool, validators: &[Validator]) {
    let cases = load_vectors(file);
    assert_eq!(
        cases.len(),
        expected_count,
        "{file}: expected {expected_count} cases, loaded {}",
        cases.len()
    );
    for case in &cases {
        for (label, validate) in validators {
            assert_eq!(
                validate(case),
                expect_valid,
                "{file} [{label}]: expected {} for {case:?}",
                if expect_valid { "valid" } else { "invalid" }
            );
        }
    }
}

fn did_validators<'a>() -> Vec<Validator<'a>> {
    vec![
        ("ensure_valid_did", &|s: &str| ensure_valid_did(s).is_ok()),
        ("ensure_valid_did_regex", &|s: &str| {
            ensure_valid_did_regex(s).is_ok()
        }),
    ]
}

fn handle_validators<'a>() -> Vec<Validator<'a>> {
    vec![
        ("ensure_valid_handle", &|s: &str| {
            ensure_valid_handle(s).is_ok()
        }),
        ("ensure_valid_handle_regex", &|s: &str| {
            ensure_valid_handle_regex(s).is_ok()
        }),
        ("is_valid_handle", &|s: &str| is_valid_handle(s)),
    ]
}

fn nsid_validators<'a>() -> Vec<Validator<'a>> {
    vec![
        ("ensure_valid_nsid", &|s: &str| ensure_valid_nsid(s).is_ok()),
        ("ensure_valid_nsid_regex", &|s: &str| {
            ensure_valid_nsid_regex(s).is_ok()
        }),
    ]
}

fn record_key_validators<'a>() -> Vec<Validator<'a>> {
    vec![
        ("ensure_valid_record_key", &|s: &str| {
            ensure_valid_record_key(s).is_ok()
        }),
        ("is_valid_record_key", &|s: &str| is_valid_record_key(s)),
    ]
}

fn tid_validators<'a>() -> Vec<Validator<'a>> {
    vec![
        ("ensure_valid_tid", &|s: &str| ensure_valid_tid(s).is_ok()),
        ("is_valid_tid", &|s: &str| is_valid_tid(s)),
    ]
}

fn datetime_validators<'a>() -> Vec<Validator<'a>> {
    vec![
        ("ensure_valid_datetime", &|s: &str| {
            ensure_valid_datetime(s).is_ok()
        }),
        ("is_valid_datetime", &|s: &str| is_valid_datetime(s)),
    ]
}

fn aturi_validators<'a>() -> Vec<Validator<'a>> {
    vec![
        ("ensure_valid_at_uri", &|s: &str| {
            ensure_valid_at_uri(s).is_ok()
        }),
        ("ensure_valid_at_uri_regex", &|s: &str| {
            ensure_valid_at_uri_regex(s).is_ok()
        }),
    ]
}

fn at_identifier_validators<'a>() -> Vec<Validator<'a>> {
    vec![
        ("ensure_valid_at_identifier", &|s: &str| {
            ensure_valid_handle(s).is_ok() || ensure_valid_did(s).is_ok()
        }),
        ("ensure_valid_at_identifier_regex", &|s: &str| {
            ensure_valid_handle_regex(s).is_ok() || ensure_valid_did_regex(s).is_ok()
        }),
    ]
}

#[test]
fn did_syntax_vectors() {
    check_vectors("did_syntax_valid.txt", 24, true, &did_validators());
    check_vectors("did_syntax_invalid.txt", 18, false, &did_validators());
}

#[test]
fn handle_syntax_vectors() {
    check_vectors("handle_syntax_valid.txt", 71, true, &handle_validators());
    check_vectors("handle_syntax_invalid.txt", 48, false, &handle_validators());
}

#[test]
fn nsid_syntax_vectors() {
    check_vectors("nsid_syntax_valid.txt", 25, true, &nsid_validators());
    check_vectors("nsid_syntax_invalid.txt", 27, false, &nsid_validators());
}

#[test]
fn record_key_syntax_vectors() {
    check_vectors(
        "recordkey_syntax_valid.txt",
        16,
        true,
        &record_key_validators(),
    );
    check_vectors(
        "recordkey_syntax_invalid.txt",
        11,
        false,
        &record_key_validators(),
    );
}

#[test]
fn tid_syntax_vectors() {
    check_vectors("tid_syntax_valid.txt", 3, true, &tid_validators());
    check_vectors("tid_syntax_invalid.txt", 7, false, &tid_validators());
}

#[test]
fn datetime_syntax_vectors() {
    check_vectors(
        "datetime_syntax_valid.txt",
        33,
        true,
        &datetime_validators(),
    );
    check_vectors(
        "datetime_syntax_invalid.txt",
        46,
        false,
        &datetime_validators(),
    );
    check_vectors(
        "datetime_parse_invalid.txt",
        6,
        false,
        &datetime_validators(),
    );
}

#[test]
fn aturi_syntax_vectors() {
    check_vectors("aturi_syntax_valid.txt", 23, true, &aturi_validators());
    check_vectors("aturi_syntax_invalid.txt", 72, false, &aturi_validators());
}

#[test]
fn at_identifier_syntax_vectors() {
    check_vectors(
        "atidentifier_syntax_valid.txt",
        11,
        true,
        &at_identifier_validators(),
    );
    check_vectors(
        "atidentifier_syntax_invalid.txt",
        22,
        false,
        &at_identifier_validators(),
    );
}
