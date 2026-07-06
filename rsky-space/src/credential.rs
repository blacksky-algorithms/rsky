//! Delegation tokens, space credentials, and client attestations
//! (proposal §Access control).
//!
//! These are structurally atproto inter-service-auth JWTs, distinguished as
//! their own credential class by the `typ` header and their claim shapes:
//!
//! - **delegation token** (`atproto-space-delegation+jwt`): minted by the
//!   user's PDS, `sub` = the space URI, `aud` = the authority space host, signed
//!   by the user's `#atproto` key. No `lxm`.
//! - **space credential** (`atproto-space-credential+jwt`): minted by the space
//!   authority in exchange for a delegation token, `sub` = the space URI, no
//!   `aud` (multi-use across repo hosts), signed by the authority space key.
//! - **client attestation** (`atproto-client-attestation+jwt`): a
//!   `private_key_jwt` client assertion the app presents when the space gates on
//!   app identity.
//!
//! This module handles the JWT envelope (encode signing input, split, verify the
//! signature against a resolved `did:key`, and validate typ/claims). Signing is
//! delegated to a caller-provided closure so this crate stays independent of any
//! particular key type; the authority/PDS wires its secp256k1/p256 key in.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::error::{Result, SpaceError};

pub const DELEGATION_TYP: &str = "atproto-space-delegation+jwt";
pub const CREDENTIAL_TYP: &str = "atproto-space-credential+jwt";
pub const ATTESTATION_TYP: &str = "atproto-client-attestation+jwt";

/// Default lifetimes (proposal defaults).
pub const DELEGATION_TTL_SECS: u64 = 60;
pub const CREDENTIAL_TTL_SECS: u64 = 7200;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtHeader {
    pub typ: String,
    pub alg: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kid: Option<String>,
}

/// Claims common to delegation tokens and space credentials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceClaims {
    /// Issuer DID (user for a delegation token, authority for a credential).
    pub iss: String,
    /// Target space URI: `at://authority/space/type/skey`.
    pub sub: String,
    /// Audience (space host) — present on delegation tokens, absent on credentials.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aud: Option<String>,
    pub iat: u64,
    pub exp: u64,
    pub jti: String,
}

/// A decoded, not-yet-signature-verified JWT.
pub struct DecodedJwt {
    pub header: JwtHeader,
    pub claims: SpaceClaims,
    /// The `header_b64.payload_b64` bytes the signature covers.
    pub signing_input: Vec<u8>,
    /// Raw signature bytes (compact r||s for ES256K/ES256).
    pub signature: Vec<u8>,
}

/// Split and decode a JWT without verifying its signature.
pub fn decode(jwt: &str) -> Result<DecodedJwt> {
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() != 3 {
        return Err(SpaceError::MalformedJwt("expected 3 segments".into()));
    }
    let header: JwtHeader = serde_json::from_slice(&b64(parts[0])?)?;
    let claims: SpaceClaims = serde_json::from_slice(&b64(parts[1])?)?;
    let signature = b64(parts[2])?;
    let signing_input = format!("{}.{}", parts[0], parts[1]).into_bytes();
    Ok(DecodedJwt {
        header,
        claims,
        signing_input,
        signature,
    })
}

/// Encode header+claims to the `header_b64.payload_b64` signing input.
pub fn signing_input(header: &JwtHeader, claims: &SpaceClaims) -> Result<String> {
    let h = URL_SAFE_NO_PAD.encode(serde_json::to_vec(header)?);
    let c = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims)?);
    Ok(format!("{h}.{c}"))
}

/// Assemble a signed JWT: `sign` receives the signing-input bytes and returns
/// the compact (r||s) signature bytes for the issuer's key.
pub fn encode<F>(header: &JwtHeader, claims: &SpaceClaims, sign: F) -> Result<String>
where
    F: FnOnce(&[u8]) -> std::result::Result<Vec<u8>, String>,
{
    let input = signing_input(header, claims)?;
    let sig = sign(input.as_bytes()).map_err(SpaceError::Crypto)?;
    Ok(format!("{input}.{}", URL_SAFE_NO_PAD.encode(sig)))
}

/// Verify a JWT's signature against a resolved `did:key` signing key.
pub fn verify_signature(decoded: &DecodedJwt, did_key: &str) -> Result<()> {
    let ok = rsky_crypto::verify::verify_signature(
        &did_key.to_string(),
        &decoded.signing_input,
        &decoded.signature,
        None,
    )
    .map_err(|e| SpaceError::Crypto(e.to_string()))?;
    if ok {
        Ok(())
    } else {
        Err(SpaceError::BadSignature)
    }
}

/// Verify a delegation token end-to-end (envelope only; the caller resolves the
/// user's signing key and provides `now`):
///
/// - `typ` is the delegation type
/// - `sub` matches the requested space URI
/// - `aud` matches `{authority}#atproto_space_host`
/// - not expired
/// - signature verifies against the user's `did:key`
///
/// Returns the user DID (`iss`) on success.
pub fn verify_delegation_token(
    jwt: &str,
    space_uri: &str,
    authority_did: &str,
    user_signing_key: &str,
    now: u64,
) -> Result<String> {
    let decoded = decode(jwt)?;
    if decoded.header.typ != DELEGATION_TYP {
        return Err(SpaceError::InvalidClaim(format!(
            "typ {} != {DELEGATION_TYP}",
            decoded.header.typ
        )));
    }
    if decoded.claims.sub != space_uri {
        return Err(SpaceError::InvalidClaim("sub != space".into()));
    }
    let want_aud = format!("{authority_did}#atproto_space_host");
    if decoded.claims.aud.as_deref() != Some(want_aud.as_str()) {
        return Err(SpaceError::InvalidClaim("aud != space host".into()));
    }
    if now >= decoded.claims.exp {
        return Err(SpaceError::Expired);
    }
    verify_signature(&decoded, user_signing_key)?;
    Ok(decoded.claims.iss)
}

/// Verify a space credential presented by a syncer to a repo host:
///
/// - `typ` is the credential type
/// - `sub` matches the space URI
/// - `iss` is the authority DID
/// - not expired
/// - signature verifies against the authority's space `did:key`
pub fn verify_space_credential(
    jwt: &str,
    space_uri: &str,
    authority_did: &str,
    authority_space_key: &str,
    now: u64,
) -> Result<()> {
    let decoded = decode(jwt)?;
    if decoded.header.typ != CREDENTIAL_TYP {
        return Err(SpaceError::InvalidClaim("wrong typ".into()));
    }
    if decoded.claims.sub != space_uri {
        return Err(SpaceError::InvalidClaim("sub != space".into()));
    }
    if decoded.claims.iss != authority_did {
        return Err(SpaceError::InvalidClaim("iss != authority".into()));
    }
    if now >= decoded.claims.exp {
        return Err(SpaceError::Expired);
    }
    verify_signature(&decoded, authority_space_key)
}

fn b64(s: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(s)
        .map_err(|e| SpaceError::MalformedJwt(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let header = JwtHeader {
            typ: CREDENTIAL_TYP.to_string(),
            alg: "ES256K".to_string(),
            kid: Some("#atproto_space".to_string()),
        };
        let claims = SpaceClaims {
            iss: "did:plc:authority".to_string(),
            sub: "at://did:plc:authority/space/community.blacksky.feed/main".to_string(),
            aud: None,
            iat: 1000,
            exp: 8200,
            jti: "nonce123".to_string(),
        };
        // Deterministic fake signer: 64 zero bytes.
        let jwt = encode(&header, &claims, |_| Ok(vec![0u8; 64])).unwrap();
        let decoded = decode(&jwt).unwrap();
        assert_eq!(decoded.header.typ, CREDENTIAL_TYP);
        assert_eq!(decoded.claims.sub, claims.sub);
        assert_eq!(decoded.claims.iss, claims.iss);
        assert_eq!(decoded.signature, vec![0u8; 64]);
    }

    #[test]
    fn malformed_jwt_rejected() {
        assert!(matches!(decode("not.a"), Err(SpaceError::MalformedJwt(_))));
    }

    #[test]
    fn delegation_claim_checks() {
        // Build a delegation token with a fake signer, then exercise the
        // envelope validation (claims/typ/exp) up to the signature step.
        let header = JwtHeader {
            typ: DELEGATION_TYP.to_string(),
            alg: "ES256K".to_string(),
            kid: Some("#atproto".to_string()),
        };
        let space = "at://did:plc:authority/space/community.blacksky.feed/main";
        let claims = SpaceClaims {
            iss: "did:plc:user".to_string(),
            sub: space.to_string(),
            aud: Some("did:plc:authority#atproto_space_host".to_string()),
            iat: 1000,
            exp: 1060,
            jti: "n".to_string(),
        };
        let jwt = encode(&header, &claims, |_| Ok(vec![0u8; 64])).unwrap();

        // Expired (now >= exp) is caught before signature verification.
        assert!(matches!(
            verify_delegation_token(&jwt, space, "did:plc:authority", "did:key:whatever", 2000),
            Err(SpaceError::Expired)
        ));
        // Wrong space is caught.
        assert!(matches!(
            verify_delegation_token(
                &jwt,
                "at://x/space/y/z",
                "did:plc:authority",
                "did:key:x",
                1000
            ),
            Err(SpaceError::InvalidClaim(_))
        ));
    }
}
