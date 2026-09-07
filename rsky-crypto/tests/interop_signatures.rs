//! Table-driven check against the upstream atproto interop crypto fixtures,
//! vendored verbatim from `interop-test-files/crypto/signature-fixtures.json`.

use base64::engine::general_purpose::STANDARD_NO_PAD;
use base64::Engine;
use rsky_crypto::verify::{verify_signature, verify_signature_digest};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    comment: String,
    message_base64: String,
    algorithm: String,
    public_key_did: String,
    signature_base64: String,
    valid_signature: bool,
    tags: Vec<String>,
}

/// The fixtures are unpadded but use the standard alphabet, not the url-safe
/// one, so fold `-_` onto `+/` before decoding.
fn decode(value: &str) -> Vec<u8> {
    let normalized: String = value
        .chars()
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            other => other,
        })
        .collect();
    STANDARD_NO_PAD.decode(normalized).unwrap()
}

fn fixtures() -> Vec<Fixture> {
    serde_json::from_str(include_str!("interop/signature-fixtures.json")).unwrap()
}

#[test]
fn interop_fixtures_match_expected_validity() {
    let fixtures = fixtures();
    assert_eq!(fixtures.len(), 6);
    assert_eq!(
        fixtures.iter().filter(|f| f.valid_signature).count(),
        2,
        "fixture set should carry exactly two valid signatures"
    );
    assert_eq!(
        fixtures
            .iter()
            .filter(|f| f.tags.iter().any(|t| t == "high-s"))
            .count(),
        2
    );
    assert_eq!(
        fixtures
            .iter()
            .filter(|f| f.tags.iter().any(|t| t == "der-encoded"))
            .count(),
        2
    );

    for fixture in &fixtures {
        let message = decode(&fixture.message_base64);
        let sig = decode(&fixture.signature_base64);
        let digest = Sha256::digest(&message);
        let actual =
            verify_signature_digest(&fixture.public_key_did, &digest, &sig, None).unwrap_or(false);
        assert_eq!(
            actual, fixture.valid_signature,
            "{} ({})",
            fixture.comment, fixture.algorithm
        );
    }
}

/// Pins the documented asymmetry of the raw-message entrypoint: p256 hashes
/// `data` internally while secp256k1 requires `data` to already be the digest.
#[test]
fn raw_message_entrypoint_semantics_are_asymmetric() {
    for fixture in fixtures().iter().filter(|f| f.valid_signature) {
        let message = decode(&fixture.message_base64);
        let sig = decode(&fixture.signature_base64);
        let digest = Sha256::digest(&message);
        let did = &fixture.public_key_did;

        match fixture.algorithm.as_str() {
            "ES256" => {
                assert!(verify_signature(did, &message, &sig, None).unwrap());
                assert!(!verify_signature(did, &digest, &sig, None).unwrap());
            }
            "ES256K" => {
                assert!(verify_signature(did, &digest, &sig, None).unwrap());
                assert!(verify_signature(did, &message, &sig, None).is_err());
            }
            other => panic!("unexpected algorithm {other}"),
        }
    }
}
