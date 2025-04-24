use crate::jwk::Jwk;
use crate::oauth_provider::oidc::sub::Sub;
use crate::oauth_types::{OAuthClientId, OAuthScope};
use biscuit::jwa::{Algorithm, SignatureAlgorithm};
use biscuit::jwk::JWK;
use biscuit::Empty;
use chrono::NaiveDate;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
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
    pub jwk: Option<JWK<Empty>>,

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

pub fn algorithm_as_string(alg: Algorithm) -> String {
    match alg {
        Algorithm::Signature(SignatureAlgorithm::HS256) => String::from(HS256_STR),
        Algorithm::Signature(SignatureAlgorithm::HS384) => String::from(HS384_STR),
        Algorithm::Signature(SignatureAlgorithm::HS512) => String::from(HS512_STR),
        Algorithm::Signature(SignatureAlgorithm::ES256) => String::from(ES256_STR),
        Algorithm::Signature(SignatureAlgorithm::ES384) => String::from(ES384_STR),
        Algorithm::Signature(SignatureAlgorithm::RS256) => String::from(RS256_STR),
        Algorithm::Signature(SignatureAlgorithm::RS384) => String::from(RS384_STR),
        Algorithm::Signature(SignatureAlgorithm::RS512) => String::from(RS512_STR),
        Algorithm::Signature(SignatureAlgorithm::PS256) => String::from(PS256_STR),
        Algorithm::Signature(SignatureAlgorithm::PS384) => String::from(PS384_STR),
        Algorithm::Signature(SignatureAlgorithm::PS512) => String::from(PS512_STR),
        _ => {
            panic!()
        }
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
    pub exp: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nbf: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,

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
    pub authorization_details: Option<Vec<AuthorizationDetail>>,

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
        let result = data.split(".").collect::<Vec<&str>>();
        if result.len() != 3 {
            return Err(SignedJwtError::InvalidFormat);
        }
        Ok(Self(data))
    }

    pub fn val(&self) -> String {
        self.0.clone()
    }
}
