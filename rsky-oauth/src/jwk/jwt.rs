use crate::jwk::Jwk;
use crate::oauth_provider::oidc::sub::Sub;
use crate::oauth_provider::token::token_id::TokenId;
use crate::oauth_types::{OAuthAuthorizationDetails, OAuthClientId, OAuthScope};
use biscuit::errors::ValidationError;
use biscuit::jwa::{Algorithm, SignatureAlgorithm};
use biscuit::jwk::{AlgorithmParameters, JWKSet, JWK};
use biscuit::jws::{Header, Secret};
use biscuit::{CompactPart, Validation};
use chrono::NaiveDate;
use once_cell::sync::Lazy;
use regex::Regex;
use rocket::form::validate::Len;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::digest::typenum::private::Invert;
use std::fmt;
use thiserror::Error;
// We'll need this for the jwk field

pub const HS256_STR: &str = "HS256";
pub const HS384_STR: &str = "HS384";
pub const HS512_STR: &str = "HS512";
pub const ES256_STR: &str = "ES256";
pub const ES384_STR: &str = "ES384";
pub const RS256_STR: &str = "RS256";
pub const RS384_STR: &str = "RS384";
pub const RS512_STR: &str = "RS512";
pub const PS256_STR: &str = "PS256";
pub const PS384_STR: &str = "PS384";
pub const PS512_STR: &str = "PS512";
pub const EDDSA_STR: &str = "EdDSA";
pub const ES512: &str = "ES512";

/// Error type for JWT operations.
#[derive(Debug, Error)]
pub enum JwtError {
    #[error("Invalid JWT format")]
    InvalidFormat,
    #[error("JWT validation error")]
    Validation,
    #[error("Invalid JWT claims: {0}")]
    InvalidClaims(String),
}

/// Standard JWT header fields, plus some custom fields
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct JwtHeader {
    /// Algorithm used to sign the token
    pub alg: Option<String>,

    /// JWT Set URL, optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jku: Option<String>,

    /// JSON Web Key, optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwk: Option<JwkFields>,

    /// Key ID, optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kid: Option<String>,

    /// X.509 URL, optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x5u: Option<String>,

    /// X.509 Certificate Chain, optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x5c: Option<Vec<String>>,

    /// X.509 Thumbprint (SHA-1), optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x5t: Option<String>,

    /// X.509 Thumbprint (SHA-256), optional
    #[serde(rename = "x5t#S256", skip_serializing_if = "Option::is_none")]
    pub x5t_s256: Option<String>,

    /// Type, optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typ: Option<String>,

    /// Content Type, optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cty: Option<String>,

    /// Critical claims, optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crit: Option<Vec<String>>,
}

/// Simplified JWK fields that can appear in JWT header
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JwkFields {
    pub kty: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crv: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub e: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<String>,
}

/// JWT Claims set with standard and custom claims
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JwtClaims {
    /// Issuer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,

    /// Subject
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,

    /// Audience
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aud: Option<Audience>,

    /// Expiration time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<i64>,

    /// Not before time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nbf: Option<i64>,

    /// Issued at time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<i64>,

    /// JWT ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,

    /// Additional custom claims
    #[serde(flatten)]
    pub additional_claims: std::collections::HashMap<String, serde_json::Value>,
}

/// JWT audience - can be a single string or array of strings
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Audience {
    Single(String),
    Multiple(Vec<String>),
}

/// A validated JWT token string
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JwtToken(String);

pub fn algorithm_as_string(alg: SignatureAlgorithm) -> String {
    match alg {
        SignatureAlgorithm::HS256 => String::from(HS256_STR),
        SignatureAlgorithm::HS384 => String::from(HS384_STR),
        SignatureAlgorithm::HS512 => String::from(HS512_STR),
        SignatureAlgorithm::ES256 => String::from(ES256_STR),
        SignatureAlgorithm::ES384 => String::from(ES384_STR),
        SignatureAlgorithm::RS256 => String::from(RS256_STR),
        SignatureAlgorithm::RS384 => String::from(RS384_STR),
        SignatureAlgorithm::RS512 => String::from(RS512_STR),
        SignatureAlgorithm::PS256 => String::from(PS256_STR),
        SignatureAlgorithm::PS384 => String::from(PS384_STR),
        SignatureAlgorithm::PS512 => String::from(PS512_STR),
        SignatureAlgorithm::None => String::from(""),
        SignatureAlgorithm::ES512 => String::from(ES512),
    }
}

impl JwtToken {
    /// Create a new JWT token
    pub fn new(token: impl Into<String>) -> Result<Self, JwtError> {
        let token = token.into();
        if token.chars().filter(|&c| c == '.').count() != 2 {
            return Err(JwtError::InvalidFormat);
        }
        Ok(Self(token))
    }

    // /// Sign claims with the given key and create a new token
    // pub fn sign(header: &JwtHeader, claims: &JwtClaims, key: &[u8]) -> Result<Self, JwtError> {
    //     let token = jsonwebtoken::encode(header, claims, &EncodingKey::from_secret(key))?;
    //     Ok(Self(token))
    // }
    //
    // /// Verify token signature and decode claims with default validation
    // pub fn verify(&self, key: &[u8]) -> Result<(JwtHeader, JwtClaims), JwtError> {
    //     self.verify_with_options(key, &Validation::default())
    // }

    // /// Verify token signature and decode claims with custom validation options.
    // ///
    // /// This method allows customizing the validation criteria for JWT verification,
    // /// which is particularly useful for validating client assertion JWTs as described
    // /// in the OAuth 2.0 specification.
    // ///
    // /// # Arguments
    // /// * `key` - The key bytes used to verify the signature
    // /// * `validation` - Custom validation parameters
    // ///
    // /// # Returns
    // /// A tuple containing the decoded header and claims on success
    // ///
    // /// # Errors
    // /// Returns a `JwtError` if verification fails
    // pub fn verify_with_options(
    //     &self,
    //     key: &[u8],
    //     validation: &Validation,
    // ) -> Result<(JwtHeader, JwtClaims), JwtError> {
    //     let token_data =
    //         jsonwebtoken::decode::<JwtClaims>(&self.0, &DecodingKey::from_secret(key), validation)?;
    //
    //     let alg = algorithm_as_string(token_data.header.alg);
    //     Ok((
    //         JwtHeader {
    //             alg: Some(alg),
    //             jku: token_data.header.jku,
    //             jwk: None, // TODO: convert from raw JWK
    //             kid: token_data.header.kid,
    //             x5u: None,
    //             x5c: None,
    //             x5t: None,
    //             x5t_s256: None,
    //             typ: token_data.header.typ,
    //             cty: token_data.header.cty,
    //             crit: None,
    //         },
    //         token_data.claims,
    //     ))
    // }

    /// Get the underlying token string
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for JwtToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Validation regex patterns
static BIRTHDATE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap());
static ZONEINFO_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[A-Za-z0-9_/]+$").unwrap());
static LOCALE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-z]{2}(-[A-Z]{2})?$").unwrap());

/// Standard JWT payload/claims with optional fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct JwtPayload {
    // Standard claims
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aud: Option<Audience>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub: Option<Sub>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nbf: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jti: Option<TokenId>,

    // Additional standard claims
    #[serde(skip_serializing_if = "Option::is_none")]
    pub htm: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub htu: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ath: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub azp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amr: Option<Vec<String>>,

    // Confirmation claims (RFC 7800)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cnf: Option<JwtConfirmation>,

    // OAuth 2.0 claims
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<OAuthClientId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<OAuthScope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,

    // Token hash claims
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub c_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub s_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_time: Option<i64>,

    // OpenID Connect profile scope claims
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub given_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub middle_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gender: Option<String>,
    // TODO@: Need to validate if this value is a URL, if exists
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<String>,
    // TODO@: Need to validate if this value is a URL, if exists
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    // TODO@: Need to validate if this value is a URL, if exists
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub birthdate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zoneinfo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,

    // OpenID Connect email scope claims
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_verified: Option<bool>,

    // OpenID Connect phone scope claims
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone_number_verified: Option<bool>,

    // OpenID Connect address scope claims
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<Address>,

    // Authorization details (RFC 9396)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_details: Option<OAuthAuthorizationDetails>,

    // Additional custom claims
    #[serde(flatten)]
    pub additional_claims: std::collections::HashMap<String, serde_json::Value>,
}
/// JWT Confirmation object (RFC 7800)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JwtConfirmation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwk: Option<Jwk>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwe: Option<String>,
    // TODO@: Need to validate if this value is a URL, if exists
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jku: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jkt: Option<String>,
    #[serde(rename = "x5t#S256", skip_serializing_if = "Option::is_none")]
    pub x5t_s256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub osc: Option<String>,
}

/// OpenID Connect Address claim
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Address {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub formatted: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub street_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locality: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub postal_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
}

/// Authorization Details (RFC 9396)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizationDetail {
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locations: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actions: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub datatypes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub privileges: Option<Vec<String>>,
    #[serde(flatten)]
    pub additional_fields: std::collections::HashMap<String, serde_json::Value>,
}

impl JwtPayload {
    /// Validate format of optional date/time/locale fields
    pub fn validate(&self) -> Result<(), JwtValidationError> {
        // Validate birthdate format (YYYY-MM-DD)
        if let Some(ref birthdate) = self.birthdate {
            if !BIRTHDATE_RE.is_match(birthdate) {
                return Err(JwtValidationError::InvalidBirthdate);
            }
            // Additional validation that it's a valid date
            if NaiveDate::parse_from_str(birthdate, "%Y-%m-%d").is_err() {
                return Err(JwtValidationError::InvalidBirthdate);
            }
        }

        // Validate zoneinfo format
        if let Some(ref zoneinfo) = self.zoneinfo {
            if !ZONEINFO_RE.is_match(zoneinfo) {
                return Err(JwtValidationError::InvalidZoneinfo);
            }
        }

        // Validate locale format
        if let Some(ref locale) = self.locale {
            if !LOCALE_RE.is_match(locale) {
                return Err(JwtValidationError::InvalidLocale);
            }
        }

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum JwtValidationError {
    #[error("Invalid birthdate format, must be YYYY-MM-DD")]
    InvalidBirthdate,
    #[error("Invalid zoneinfo format")]
    InvalidZoneinfo,
    #[error("Invalid locale format")]
    InvalidLocale,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn test_jwt_payload_validation() {
        let mut payload = JwtPayload::default();
        payload.birthdate = Some("2000-01-01".to_string());
        payload.zoneinfo = Some("America/New_York".to_string());
        payload.locale = Some("en-US".to_string());

        // Valid formats
        assert!(payload.validate().is_ok());

        // Invalid birthdate
        payload.birthdate = Some("2000-13-01".to_string());
        assert!(matches!(
            payload.validate(),
            Err(JwtValidationError::InvalidBirthdate)
        ));

        // Invalid zoneinfo
        payload.birthdate = Some("2000-01-01".to_string());
        payload.zoneinfo = Some("America/New York".to_string());
        assert!(matches!(
            payload.validate(),
            Err(JwtValidationError::InvalidZoneinfo)
        ));

        // Invalid locale
        payload.zoneinfo = Some("America/New_York".to_string());
        payload.locale = Some("invalid".to_string());
        assert!(matches!(
            payload.validate(),
            Err(JwtValidationError::InvalidLocale)
        ));
    }

    #[test]
    fn test_serialization() {
        let mut payload = JwtPayload {
            iss: Some("issuer".to_string()),
            sub: Some(Sub::new("subject").unwrap()),
            aud: Some(Audience::Single("audience".to_string())),
            birthdate: Some("2000-01-01".to_string()),
            ..Default::default()
        };

        // Add a custom claim
        payload.additional_claims.insert(
            "custom_claim".to_string(),
            serde_json::Value::String("value".to_string()),
        );

        let serialized = serde_json::to_string(&payload).unwrap();
        let deserialized: JwtPayload = serde_json::from_str(&serialized).unwrap();

        assert_eq!(payload, deserialized);
        assert_eq!(
            deserialized.additional_claims.get("custom_claim").unwrap(),
            &serde_json::Value::String("value".to_string())
        );
    }

    #[test]
    fn test_jwt_token_validation() {
        let invalid = JwtToken::new("invalid");
        assert!(matches!(invalid, Err(JwtError::InvalidFormat)));

        let valid = JwtToken::new("header.payload.signature");
        assert!(valid.is_ok());
    }

    // #[test]
    // fn test_jwt_sign_verify() {
    //     let key = b"secret";
    //     let now = SystemTime::now()
    //         .duration_since(UNIX_EPOCH)
    //         .unwrap()
    //         .as_secs() as i64;
    //
    //     let mut claims = JwtClaims {
    //         iss: Some("test-issuer".to_string()),
    //         sub: Some("test-subject".to_string()),
    //         aud: Some(Audience::Single("test-audience".to_string())),
    //         exp: Some(now + 3600),
    //         nbf: None,
    //         iat: Some(now),
    //         jti: None,
    //         additional_claims: std::collections::HashMap::new(),
    //     };
    //
    //     claims.additional_claims.insert(
    //         "custom".to_string(),
    //         serde_json::Value::String("value".to_string()),
    //     );
    //
    //     let token = JwtToken::sign(&Header::default(), &claims, key).unwrap();
    //
    //     // Create a custom validation that matches our test JWT
    //     let mut validation = Validation::default();
    //     validation.set_audience(&["test-audience"]);
    //
    //     // Verify with our specific validation
    //     let (_header, decoded_claims) = token.verify_with_options(key, &validation).unwrap();
    //     assert_eq!(decoded_claims.iss, claims.iss);
    //     assert_eq!(decoded_claims.sub, claims.sub);
    //     assert_eq!(
    //         decoded_claims.additional_claims["custom"],
    //         serde_json::json!("value")
    //     );
    // }
    //
    // #[test]
    // fn test_jwt_verify_no_validation() {
    //     let key = b"secret";
    //     let now = SystemTime::now()
    //         .duration_since(UNIX_EPOCH)
    //         .unwrap()
    //         .as_secs() as i64;
    //
    //     let mut claims = JwtClaims {
    //         iss: Some("test-issuer".to_string()),
    //         sub: Some("test-subject".to_string()),
    //         aud: Some(Audience::Single("test-audience".to_string())),
    //         exp: Some(now + 3600),
    //         nbf: None,
    //         iat: Some(now),
    //         jti: None,
    //         additional_claims: std::collections::HashMap::new(),
    //     };
    //
    //     claims.additional_claims.insert(
    //         "custom".to_string(),
    //         serde_json::Value::String("value".to_string()),
    //     );
    //
    //     let token = JwtToken::sign(&Header::default(), &claims, key).unwrap();
    //
    //     // Create a validation with all checks disabled for testing
    //     let mut validation = Validation::default();
    //     validation.validate_exp = false;
    //     validation.validate_nbf = false;
    //     validation.validate_aud = false;
    //
    //     // Verify with relaxed validation
    //     let (_header, decoded_claims) = token.verify_with_options(key, &validation).unwrap();
    //     assert_eq!(decoded_claims.iss, claims.iss);
    //     assert_eq!(decoded_claims.sub, claims.sub);
    //     assert_eq!(
    //         decoded_claims.additional_claims["custom"],
    //         serde_json::json!("value")
    //     );
    // }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SignedJwt(String);

/// Errors that can occur when working with token identification.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SignedJwtError {
    #[error("Invalid format")]
    InvalidFormat,
}

impl SignedJwt {
    pub fn new(data: impl Into<String>) -> Result<Self, SignedJwtError> {
        let data = data.into();
        Ok(Self(data))
    }

    pub fn val(&self) -> String {
        self.0.clone()
    }
}

pub fn decode_with_jwk<T, H, J>(
    jwt: biscuit::jws::Compact<T, JwtHeader>,
    jwk: JWK<J>,
    expected_algorithm: Option<SignatureAlgorithm>,
) -> Result<biscuit::jws::Compact<T, H>, biscuit::errors::Error>
where
    T: CompactPart,
    H: Serialize + DeserializeOwned,
{
    match jwt {
        biscuit::jws::Compact::Decoded { .. } => Err(biscuit::errors::Error::UnsupportedOperation),
        biscuit::jws::Compact::Encoded(encoded) => {
            if encoded.len() != 3 {
                unimplemented!()
                // Err(DecodeError::PartsLengthError {
                //     actual: encoded.len(),
                //     expected: 3,
                // })?
            }

            let signature: Vec<u8> = encoded.part(2)?;
            let payload = &encoded.parts[0..2].join(".");

            let header: Header<H> = encoded.part(0)?;
            let algorithm = match jwk.common.algorithm {
                Some(jwk_alg) => {
                    let algorithm = match jwk_alg {
                        Algorithm::Signature(algorithm) => algorithm,
                        _ => Err(ValidationError::UnsupportedKeyAlgorithm)?,
                    };

                    if header.registered.algorithm != algorithm {
                        Err(ValidationError::WrongAlgorithmHeader)?;
                    }

                    if let Some(expected_algorithm) = expected_algorithm {
                        if expected_algorithm != algorithm {
                            Err(ValidationError::WrongAlgorithmHeader)?;
                        }
                    }

                    algorithm
                }
                None => match expected_algorithm {
                    Some(expected_algorithm) => {
                        if expected_algorithm != header.registered.algorithm {
                            Err(ValidationError::WrongAlgorithmHeader)?;
                        }
                        expected_algorithm
                    }
                    None => Err(ValidationError::MissingAlgorithm)?,
                },
            };

            let secret = match &jwk.algorithm {
                AlgorithmParameters::EllipticCurve(ec) => ec.jws_public_key_secret(),
                AlgorithmParameters::RSA(rsa) => rsa.jws_public_key_secret(),
                AlgorithmParameters::OctetKey(oct) => Secret::Bytes(oct.value.clone()),
                _ => Err(ValidationError::UnsupportedKeyAlgorithm)?,
            };

            algorithm
                .verify(signature.as_ref(), payload.as_ref(), &secret)
                .map_err(|_| ValidationError::InvalidSignature)?;

            let decoded_claims: T = encoded.part(1)?;

            Ok(biscuit::jws::Compact::new_decoded(header, decoded_claims))
        }
    }
}
