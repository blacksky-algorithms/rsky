//! HTTP Message Signatures for space credentials (proposal §Presenting a space
//! credential).
//!
//! A space credential bound by `cnf.kid` is presented as
//! `Authorization: Atproto-Space-Credential <credential>`, beside an
//! `Atproto-Space-Audience` header and an [RFC 9421] signature by the bound key
//! over both. The proposal fixes the profile:
//!
//! - the signature label is `atproto-space-credential`, in both the
//!   `Signature-Input` and `Signature` dictionaries; other labels are ignored,
//!   and a label named twice is refused rather than resolved;
//! - the covered components are exactly `("authorization"
//!   "atproto-space-audience")`, in that order;
//! - the parameters carry `tag="atproto-space-credential"`; an `alg` parameter
//!   is optional and must name `ecdsa-p256-sha256`, and a `keyid` parameter is
//!   optional and must equal `cnf.kid`;
//! - the signature is the 64-byte `r || s` of `ecdsa-p256-sha256`, verified
//!   against `cnf.kid`, which must be a P-256 `did:key`. High-S is accepted.
//!
//! The signature base is the two components and `@signature-params`, joined by
//! LF with no trailing LF:
//!
//! ```text
//! "authorization": Atproto-Space-Credential eyJ...
//! "atproto-space-audience": did:plc:repoowner
//! "@signature-params": ("authorization" "atproto-space-audience");tag="atproto-space-credential"
//! ```
//!
//! Each component is its header value trimmed of surrounding whitespace and
//! nothing else, the scheme's case included. `@signature-params` is the
//! canonical [RFC 8941] serialization of the inner list the request carried,
//! rebuilt rather than copied, as the reference does.
//!
//! The signature binds neither method nor URL, and carries no nonce or
//! `created`: the proposal allows reusing one for the same credential and
//! audience until the credential expires. Checking that the audience is the
//! right one for the request is the caller's job, as is recognising the scheme
//! and requiring each of `Authorization` and `Atproto-Space-Audience` to
//! arrive exactly once.
//!
//! This follows the profile as `atproto-space`'s `http_signature.rs`
//! (`atproto-crates`, MIT) implements it, so the two verify the same bytes.
//!
//! [RFC 9421]: https://www.rfc-editor.org/rfc/rfc9421
//! [RFC 8941]: https://www.rfc-editor.org/rfc/rfc8941

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use p256::ecdsa::signature::{Signer, Verifier};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use sfv::{BareItem, FieldType as _, InnerList, Item, ListEntry, Parameters, Parser};

use crate::error::{Result, SpaceError};

pub const SIGNATURE_LABEL: &str = "atproto-space-credential";
pub const SIGNATURE_TAG: &str = "atproto-space-credential";
pub const SIGNATURE_ALG: &str = "ecdsa-p256-sha256";
pub const AUTH_SCHEME: &str = "Atproto-Space-Credential";
pub const AUDIENCE_HEADER: &str = "atproto-space-audience";
pub const SIGNATURE_INPUT_HEADER: &str = "signature-input";
pub const SIGNATURE_HEADER: &str = "signature";

const AUTHORIZATION_COMPONENT: &str = "authorization";
const COVERED: [&str; 2] = [AUTHORIZATION_COMPONENT, AUDIENCE_HEADER];
const SIGNATURE_LEN: usize = 64;

/// The header values a verifier needs, one each. `Signature-Input` and
/// `Signature` are dictionaries, so a request may split either across field
/// lines; pass them joined with `", "` in the order received (RFC 9110 §5.3).
/// An absent header is the empty string, which verification refuses.
#[derive(Debug, Clone, Copy)]
pub struct SignedRequest<'a> {
    pub authorization: &'a str,
    pub audience: &'a str,
    pub signature_input: &'a str,
    pub signature: &'a str,
}

/// The headers a client sets to use a credential.
#[derive(Clone, PartialEq, Eq)]
pub struct CredentialHeaders {
    pub authorization: String,
    pub audience: String,
    pub signature_input: String,
    pub signature: String,
}

impl std::fmt::Debug for CredentialHeaders {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialHeaders")
            .field("authorization", &format_args!("{AUTH_SCHEME} <redacted>"))
            .field("audience", &self.audience)
            .field("signature_input", &self.signature_input)
            .field("signature", &self.signature)
            .finish()
    }
}

/// The credential in an `Authorization` value, when its scheme is
/// `Atproto-Space-Credential`. The scheme is recognised in any case.
pub fn credential_from_authorization(authorization: &str) -> Option<&str> {
    let (scheme, credential) = authorization.trim().split_once(' ')?;
    if !scheme.eq_ignore_ascii_case(AUTH_SCHEME) {
        return None;
    }
    let credential = credential.trim();
    (!credential.is_empty()).then_some(credential)
}

/// Sign a request that uses `credential` against `audience`, as the reference
/// client does: no `alg` or `keyid`, and a padded base64 `Signature`.
pub fn sign_credential_use(
    credential: &str,
    audience: &str,
    signing_key: &SigningKey,
) -> CredentialHeaders {
    let authorization = format!("{AUTH_SCHEME} {credential}");
    let mut params = Parameters::new();
    params.insert(
        sfv::key_ref("tag").to_owned(),
        BareItem::String(sfv::string_ref(SIGNATURE_TAG).to_owned()),
    );
    let params = serialize_inner_list(&covered_list(params));
    let base = signature_base(&authorization, audience, &params);
    let signature: Signature = signing_key.sign(&base);
    CredentialHeaders {
        authorization,
        audience: audience.to_string(),
        signature_input: format!("{SIGNATURE_LABEL}={params}"),
        signature: format!(
            "{SIGNATURE_LABEL}=:{}:",
            STANDARD.encode(signature.to_bytes())
        ),
    }
}

/// Verify a request's credential-use signature against `cnf_kid`.
///
/// Checks every rule in the module docs except the audience's correctness for
/// the call and the `Authorization` scheme, which are the caller's.
pub fn verify_credential_use(request: &SignedRequest<'_>, cnf_kid: &str) -> Result<()> {
    if request.authorization.trim().is_empty() {
        return Err(refused("missing authorization"));
    }
    if request.audience.trim().is_empty() {
        return Err(refused("missing atproto-space-audience"));
    }
    let key = p256_verifying_key(cnf_kid)?;
    let input = match member(SIGNATURE_INPUT_HEADER, request.signature_input)? {
        ListEntry::InnerList(input) => input,
        ListEntry::Item(_) => return Err(refused("signature-input member is not an inner list")),
    };
    let signature = match member(SIGNATURE_HEADER, request.signature)? {
        ListEntry::Item(Item {
            bare_item: BareItem::ByteSequence(bytes),
            params,
        }) if params.is_empty() => bytes,
        _ => return Err(refused("signature member is not a byte sequence")),
    };

    let covered = input.items.len() == COVERED.len()
        && input.items.iter().zip(COVERED).all(|(item, expected)| {
            item.params.is_empty()
                && matches!(&item.bare_item, BareItem::String(value) if value.as_str() == expected)
        });
    if !covered {
        return Err(refused(
            "covered components must be exactly (\"authorization\" \"atproto-space-audience\")",
        ));
    }
    match input.params.get("tag") {
        Some(BareItem::String(tag)) if tag.as_str() == SIGNATURE_TAG => {}
        _ => return Err(refused("tag must be \"atproto-space-credential\"")),
    }
    match input.params.get("alg") {
        None => {}
        Some(BareItem::String(alg)) if alg.as_str() == SIGNATURE_ALG => {}
        Some(_) => return Err(refused("alg must be \"ecdsa-p256-sha256\"")),
    }
    match input.params.get("keyid") {
        None => {}
        Some(BareItem::String(keyid)) if keyid.as_str() == cnf_kid => {}
        Some(_) => return Err(refused("keyid does not match the credential's cnf.kid")),
    }

    if signature.len() != SIGNATURE_LEN {
        return Err(refused("signature must be 64 bytes"));
    }
    let signature = Signature::from_slice(&signature)
        .map_err(|_| refused("signature is not a valid r || s"))?;
    let base = signature_base(
        request.authorization,
        request.audience,
        &serialize_inner_list(&input),
    );
    key.verify(&base, &signature)
        .map_err(|_| refused("signature does not verify against cnf.kid"))
}

/// The signature base for `authorization`, `audience` and the serialized
/// `@signature-params` value.
pub fn signature_base(authorization: &str, audience: &str, signature_params: &str) -> Vec<u8> {
    [
        format!("\"{AUTHORIZATION_COMPONENT}\": {}", authorization.trim()),
        format!("\"{AUDIENCE_HEADER}\": {}", audience.trim()),
        format!("\"@signature-params\": {signature_params}"),
    ]
    .join("\n")
    .into_bytes()
}

fn refused(reason: &str) -> SpaceError {
    SpaceError::BadHttpSignature(reason.to_string())
}

/// `did_key` as a P-256 verifying key. Anything else -- another curve, or a
/// private key -- is refused.
fn p256_verifying_key(did_key: &str) -> Result<VerifyingKey> {
    let parsed = rsky_crypto::did::parse_did_key(&did_key.to_string())
        .map_err(|error| refused(&format!("cnf.kid is not a did:key: {error}")))?;
    if parsed.jwt_alg != rsky_crypto::constants::P256_JWT_ALG {
        return Err(refused("cnf.kid must be a P-256 did:key"));
    }
    VerifyingKey::from_sec1_bytes(&parsed.key_bytes)
        .map_err(|_| refused("cnf.kid is not a valid P-256 key"))
}

fn covered_list(params: Parameters) -> InnerList {
    let items = COVERED
        .iter()
        .map(|component| Item::new(BareItem::String(sfv::string_ref(component).to_owned())))
        .collect();
    InnerList::with_params(items, params)
}

fn serialize_inner_list(input: &InnerList) -> String {
    vec![ListEntry::InnerList(input.clone())]
        .serialize()
        .unwrap_or_default()
}

/// The `atproto-space-credential` member of the dictionary in header `name`.
/// Two members under that label are refused: RFC 8941 keeps the last, which
/// leaves open which one the client signed.
fn member(name: &str, value: &str) -> Result<ListEntry> {
    if value.trim().is_empty() {
        return Err(refused(&format!("missing {name}")));
    }
    let malformed = |error: sfv::Error| refused(&format!("malformed {name}: {error}"));
    let mut dictionary: sfv::Dictionary = Parser::new(value).parse().map_err(malformed)?;
    let labelled = Parser::new(value)
        .parse_dictionary_with_visitor(LabelCount(0))
        .map_err(malformed)?;
    if labelled > 1 {
        return Err(refused(&format!("{name} names {SIGNATURE_LABEL} twice")));
    }
    dictionary
        .shift_remove(SIGNATURE_LABEL)
        .ok_or_else(|| refused(&format!("{name} has no {SIGNATURE_LABEL} member")))
}

/// Counts dictionary members keyed [`SIGNATURE_LABEL`], which
/// [`sfv::Dictionary`] cannot: it keeps only the last.
struct LabelCount(usize);

impl<'de> sfv::visitor::DictionaryVisitor<'de> for LabelCount {
    type Out = usize;
    type Error = std::convert::Infallible;

    fn entry(
        &mut self,
        key: &'de sfv::KeyRef,
    ) -> std::result::Result<impl sfv::visitor::EntryVisitor<'de>, Self::Error> {
        if key.as_str() == SIGNATURE_LABEL {
            self.0 += 1;
        }
        Ok(sfv::visitor::Ignored)
    }

    fn finish(self) -> std::result::Result<usize, Self::Error> {
        Ok(self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CREDENTIAL: &str = "eyJhbGciOiJFUzI1NiJ9.eyJpc3MiOiJkaWQ6cGxjOmEifQ.c2ln";
    const AUDIENCE: &str = "did:plc:repoowner";

    fn signing_key(seed: u8) -> SigningKey {
        SigningKey::from_slice(&[seed; 32]).unwrap()
    }

    fn did_key(key: &SigningKey) -> String {
        let sec1 = key
            .verifying_key()
            .to_encoded_point(false)
            .as_bytes()
            .to_vec();
        rsky_crypto::did::format_did_key(rsky_crypto::constants::P256_JWT_ALG.to_string(), sec1)
            .unwrap()
    }

    fn request(headers: &CredentialHeaders) -> SignedRequest<'_> {
        SignedRequest {
            authorization: &headers.authorization,
            audience: &headers.audience,
            signature_input: &headers.signature_input,
            signature: &headers.signature,
        }
    }

    #[test]
    fn the_reference_header_shape() {
        let headers = sign_credential_use(CREDENTIAL, AUDIENCE, &signing_key(1));
        assert_eq!(
            headers.authorization,
            format!("Atproto-Space-Credential {CREDENTIAL}")
        );
        assert_eq!(
            headers.signature_input,
            "atproto-space-credential=(\"authorization\" \"atproto-space-audience\");tag=\"atproto-space-credential\""
        );
        assert!(headers.signature.starts_with("atproto-space-credential=:"));
    }

    #[test]
    fn a_signed_request_verifies_against_its_key() {
        let key = signing_key(1);
        let headers = sign_credential_use(CREDENTIAL, AUDIENCE, &key);
        verify_credential_use(&request(&headers), &did_key(&key)).unwrap();
    }

    #[test]
    fn another_key_does_not_verify() {
        let headers = sign_credential_use(CREDENTIAL, AUDIENCE, &signing_key(1));
        assert!(verify_credential_use(&request(&headers), &did_key(&signing_key(2))).is_err());
    }

    #[test]
    fn the_signature_covers_the_audience_and_the_authorization_as_sent() {
        let key = signing_key(1);
        let headers = sign_credential_use(CREDENTIAL, AUDIENCE, &key);
        let mut other = headers.clone();
        other.audience = "did:plc:someoneelse".to_string();
        assert!(verify_credential_use(&request(&other), &did_key(&key)).is_err());
        let mut lowered = headers.clone();
        lowered.authorization = format!("atproto-space-credential {CREDENTIAL}");
        assert!(verify_credential_use(&request(&lowered), &did_key(&key)).is_err());
    }

    #[test]
    fn the_profile_is_enforced() {
        let key = signing_key(1);
        let kid = did_key(&key);
        let signed = |input: &str| {
            let params = input.split_once('=').unwrap().1;
            let base = signature_base(&format!("{AUTH_SCHEME} {CREDENTIAL}"), AUDIENCE, params);
            let signature: Signature = key.sign(&base);
            CredentialHeaders {
                authorization: format!("{AUTH_SCHEME} {CREDENTIAL}"),
                audience: AUDIENCE.to_string(),
                signature_input: input.to_string(),
                signature: format!(
                    "{SIGNATURE_LABEL}=:{}:",
                    STANDARD.encode(signature.to_bytes())
                ),
            }
        };
        // Optional parameters, correctly valued, are accepted.
        let ok = signed(&format!(
            "{SIGNATURE_LABEL}=(\"authorization\" \"atproto-space-audience\");tag=\"atproto-space-credential\";alg=\"ecdsa-p256-sha256\";keyid=\"{kid}\""
        ));
        verify_credential_use(&request(&ok), &kid).unwrap();
        for bad in [
            // components reordered
            format!("{SIGNATURE_LABEL}=(\"atproto-space-audience\" \"authorization\");tag=\"atproto-space-credential\""),
            // missing tag
            format!("{SIGNATURE_LABEL}=(\"authorization\" \"atproto-space-audience\")"),
            // wrong alg
            format!("{SIGNATURE_LABEL}=(\"authorization\" \"atproto-space-audience\");tag=\"atproto-space-credential\";alg=\"ed25519\""),
            // keyid naming another key
            format!("{SIGNATURE_LABEL}=(\"authorization\" \"atproto-space-audience\");tag=\"atproto-space-credential\";keyid=\"did:key:zOther\""),
            // the 2026-10-01 label
            "atproto-space=(\"authorization\" \"atproto-space-audience\");tag=\"atproto-space-credential\"".to_string(),
        ] {
            assert!(verify_credential_use(&request(&signed(&bad)), &kid).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_label_named_twice_is_refused() {
        let key = signing_key(1);
        let mut headers = sign_credential_use(CREDENTIAL, AUDIENCE, &key);
        headers.signature_input =
            format!("{}, {}", headers.signature_input, headers.signature_input);
        assert!(verify_credential_use(&request(&headers), &did_key(&key)).is_err());
    }

    #[test]
    fn other_labels_beside_it_are_ignored() {
        let key = signing_key(1);
        let mut headers = sign_credential_use(CREDENTIAL, AUDIENCE, &key);
        headers.signature_input = format!("other=(\"@method\"), {}", headers.signature_input);
        headers.signature = format!("other=:AAAA:, {}", headers.signature);
        verify_credential_use(&request(&headers), &did_key(&key)).unwrap();
    }

    #[test]
    fn cnf_kid_must_be_a_p256_public_did_key() {
        let headers = sign_credential_use(CREDENTIAL, AUDIENCE, &signing_key(1));
        for kid in [
            "not-a-did-key",
            "did:key:zQ3shZ32Wx1boR2waY3izFhQhFW7RRj7i5qex1joXY8dEP2Sc",
        ] {
            assert!(
                verify_credential_use(&request(&headers), kid).is_err(),
                "{kid}"
            );
        }
    }

    #[test]
    fn the_scheme_is_recognised_in_any_case() {
        assert_eq!(
            credential_from_authorization(&format!("atproto-space-credential {CREDENTIAL}")),
            Some(CREDENTIAL)
        );
        assert_eq!(
            credential_from_authorization(&format!("Bearer {CREDENTIAL}")),
            None
        );
        assert_eq!(
            credential_from_authorization("Atproto-Space-Credential "),
            None
        );
    }
}
