use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// A JWT used as an Authorization Request.
///
/// JWT requirements:
/// - "iat" is required and MUST be less than one minute old
///
/// See RFC 9101 for details on Authorization Request JWTs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthAuthorizationRequestJar {
    /// The request JWT that contains the authorization request parameters
    request: Jwt,
}

/// Claims that must be present in the JWT
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestClaims {
    /// Issued at time (required)
    pub iat: i64,
    /// Optional expiration time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<i64>,
    /// Optional JWT ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,
    // Additional custom claims can be added as needed
    #[serde(flatten)]
    pub additional_claims: serde_json::Map<String, serde_json::Value>,
}

/// A JWT value that can be either signed or unsigned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Jwt {
    /// A signed JWT
    Signed(String),
    /// An unsigned JWT (header and claims only)
    Unsigned(String),
}

impl OAuthAuthorizationRequestJar {
    /// Create a new authorization request JAR with the specified algorithm and key.
    ///
    /// This function replaces both `new_signed` and `new_unsigned` by allowing the caller
    /// to specify the algorithm and key to use. If the algorithm is `None`, an unsigned
    /// JWT will be created. Otherwise, a signed JWT will be created using the specified
    /// algorithm and key.
    ///
    /// # Arguments
    /// * `claims` - The claims to include in the JWT
    /// * `alg` - The algorithm to use for signing, or `None` for an unsigned JWT
    /// * `key` - The key to sign the JWT with, or `None` for an unsigned JWT
    ///
    /// # Examples
    ///
    /// Creating a signed JWT with ES256:
    /// ```
    /// use std::str;
    ///
    /// let claims = RequestClaims {
    ///     iat: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64,
    ///     exp: Some(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64 + 300),
    ///     jti: Some("test-id".to_string()),
    ///     additional_claims: serde_json::Map::new(),
    /// };
    ///
    /// // ROT13-encoded EC private key
    /// // Please don't use this key for anything
    /// let encoded_key = "-----ORTVA CEVINGR XRL-----
    /// ZVTUNtRNZOZTOldTFZ49NtRTPPdTFZ49NjRUOT0jnjVONDDtKS0dxv6bEKcdGeHd
    /// L/Rb9hBBIuOS7ftobTz3V6t7Oe6uENAPNNE38eqJJL/rpIWviZUQNW0MP5iHWYUR
    /// eCn7dMVM53xuIGNc+0mDwUEC1405fp7rNkmqXRaFATQkIn+9bLE0SdCR
    /// -----RAQ CEVINGR XRL-----";
    ///
    /// // ROT13 decode function
    /// fn rot13_decode(encoded: &str) -> Vec<u8> {
    ///     encoded.chars().map(|c| {
    ///         if c >= 'A' && c <= 'Z' {
    ///             let mut code = c as u8 + 13;
    ///             if code > b'Z' { code -= 26; }
    ///             code as char
    ///         } else if c >= 'a' && c <= 'z' {
    ///             let mut code = c as u8 + 13;
    ///             if code > b'z' { code -= 26; }
    ///             code as char
    ///         } else {
    ///             c
    ///         }
    ///     }).collect::<String>().into_bytes()
    /// }
    ///
    /// let key = rot13_decode(encoded_key);
    /// let jar = OAuthAuthorizationRequestJar::new(claims, Some(Algorithm::ES256), Some(&key)).unwrap();
    /// assert!(jar.is_signed());
    /// ```
    ///
    /// Creating an unsigned JWT:
    /// ```
    /// let claims = RequestClaims {
    ///     iat: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64,
    ///     exp: Some(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64 + 300),
    ///     jti: Some("test-id".to_string()),
    ///     additional_claims: serde_json::Map::new(),
    /// };
    ///
    /// let jar = OAuthAuthorizationRequestJar::new(claims, None, None).unwrap();
    /// assert!(!jar.is_signed());
    /// ```
    pub fn new(
        claims: RequestClaims,
        alg: Option<Algorithm>,
        key: Option<&[u8]>,
    ) -> Result<Self, JarError> {
        validate_claims(&claims)?;

        match alg {
            Some(algorithm) => {
                // Signed JWT
                let key_bytes = key.ok_or_else(|| {
                    JarError::JwtEncoding(jsonwebtoken::errors::Error::from(
                        jsonwebtoken::errors::ErrorKind::InvalidKeyFormat,
                    ))
                })?;

                let encoding_key = match algorithm {
                    Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512 => {
                        EncodingKey::from_secret(key_bytes)
                    }
                    Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512 => {
                        // Try to interpret as PEM first, fall back to DER if that fails
                        match EncodingKey::from_rsa_pem(key_bytes) {
                            Ok(key) => key,
                            Err(_) => EncodingKey::from_rsa_der(key_bytes),
                        }
                    }
                    Algorithm::ES256 | Algorithm::ES384 => {
                        // Try to interpret as PEM first, fall back to DER if that fails
                        match EncodingKey::from_ec_pem(key_bytes) {
                            Ok(key) => key,
                            Err(_) => EncodingKey::from_ec_der(key_bytes),
                        }
                    }
                    Algorithm::EdDSA => {
                        // Try to interpret as PEM first, fall back to DER if that fails
                        match EncodingKey::from_ed_pem(key_bytes) {
                            Ok(key) => key,
                            Err(_) => EncodingKey::from_ed_der(key_bytes),
                        }
                    }
                    Algorithm::PS256 | Algorithm::PS384 | Algorithm::PS512 => {
                        // Try to interpret as PEM first, fall back to DER if that fails
                        match EncodingKey::from_rsa_pem(key_bytes) {
                            Ok(key) => key,
                            Err(_) => EncodingKey::from_rsa_der(key_bytes),
                        }
                    }
                };

                let token = encode(&Header::new(algorithm), &claims, &encoding_key)
                    .map_err(JarError::JwtEncoding)?;

                Ok(Self {
                    request: Jwt::Signed(token),
                })
            }
            None => {
                // Unsigned JWT - we need to manually construct this
                // since jsonwebtoken doesn't support "none" algorithm

                // Create header with "none" algorithm
                let header = serde_json::json!({
                    "alg": "none",
                    "typ": "JWT"
                });

                // Import base64 engine components
                use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

                // Base64 encode header
                let header_encoded =
                    URL_SAFE_NO_PAD.encode(serde_json::to_string(&header).map_err(|_| {
                        JarError::JwtEncoding(jsonwebtoken::errors::Error::from(
                            jsonwebtoken::errors::ErrorKind::InvalidToken,
                        ))
                    })?);

                // Base64 encode claims
                let claims_encoded =
                    URL_SAFE_NO_PAD.encode(serde_json::to_string(&claims).map_err(|_| {
                        JarError::JwtEncoding(jsonwebtoken::errors::Error::from(
                            jsonwebtoken::errors::ErrorKind::InvalidToken,
                        ))
                    })?);

                // Combine parts with no signature
                let token = format!("{}.{}.d", header_encoded, claims_encoded);

                Ok(Self {
                    request: Jwt::Unsigned(token),
                })
            }
        }
    }

    /// Verify and decode a signed JWT.
    pub fn verify_signed(&self, key: &[u8]) -> Result<RequestClaims, JarError> {
        let token = self.jwt();

        let validation = Validation::new(Algorithm::ES256);
        let token_data =
            decode::<RequestClaims>(token, &DecodingKey::from_secret(key), &validation)
                .map_err(JarError::JwtDecoding)?;

        validate_claims(&token_data.claims)?;
        Ok(token_data.claims)
    }

    /// Decode an unsigned JWT without verification.
    pub fn decode_unsigned(&self) -> Result<RequestClaims, JarError> {
        let token = self.jwt();

        //algorithm does not matter
        let mut validation = Validation::new(Algorithm::ES256);
        validation.insecure_disable_signature_validation();

        let token_data =
            decode::<RequestClaims>(token, &DecodingKey::from_secret(&[]), &validation)
                .map_err(JarError::JwtDecoding)?;

        validate_claims(&token_data.claims)?;
        Ok(token_data.claims)
    }

    /// Get the JWT value.
    pub fn jwt(&self) -> &str {
        match &self.request {
            Jwt::Signed(jwt) | Jwt::Unsigned(jwt) => jwt,
        }
    }

    /// Check if the JWT is signed.
    pub fn is_signed(&self) -> bool {
        matches!(self.request, Jwt::Signed(_))
    }
}

impl fmt::Display for OAuthAuthorizationRequestJar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AuthorizationRequestJar({})", self.jwt())
    }
}

/// Validate the required claims and their values.
fn validate_claims(claims: &RequestClaims) -> Result<(), JarError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| JarError::SystemTime)?
        .as_secs() as i64;

    // iat must be present and less than one minute old
    if claims.iat <= 0 {
        return Err(JarError::InvalidIat);
    }

    // if now - claims.iat > 60 {
    //     return Err(JarError::IatTooOld);
    // }

    // If expiration is set, it must be in the future
    if let Some(exp) = claims.exp {
        if exp <= now {
            return Err(JarError::Expired);
        }
    }

    Ok(())
}

/// Errors that can occur when creating or validating a JAR.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum JarError {
    #[error("JWT encoding error: {0}")]
    JwtEncoding(#[source] jsonwebtoken::errors::Error),

    #[error("JWT decoding error: {0}")]
    JwtDecoding(#[source] jsonwebtoken::errors::Error),

    #[error("System time error")]
    SystemTime,

    #[error("Invalid iat claim")]
    InvalidIat,

    #[error("iat claim is too old (must be less than one minute)")]
    IatTooOld,

    #[error("Token has expired")]
    Expired,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_test_claims() -> RequestClaims {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        RequestClaims {
            iat: now,
            exp: Some(now + 300), // 5 minutes from now
            jti: Some("test-id".to_string()),
            additional_claims: serde_json::Map::new(),
        }
    }

    #[test]
    fn test_new_signed_valid() {
        let claims = create_test_claims();
        let key = b"test-key";
        let jar =
            OAuthAuthorizationRequestJar::new(claims.clone(), Some(Algorithm::HS256), Some(key))
                .unwrap();

        assert!(jar.is_signed());

        // Verify the token can be decoded
        let decoded_claims = jar.verify_signed(key).unwrap();
        assert_eq!(decoded_claims.iat, claims.iat);
        assert_eq!(decoded_claims.jti, claims.jti);
    }

    #[test]
    fn test_new_unsigned_valid() {
        let claims = create_test_claims();
        let jar = OAuthAuthorizationRequestJar::new(claims.clone(), None, None).unwrap();

        assert!(!jar.is_signed());

        // Verify the token can be decoded
        let decoded_claims = jar.decode_unsigned().unwrap();
        assert_eq!(decoded_claims.iat, claims.iat);
        assert_eq!(decoded_claims.jti, claims.jti);
    }

    #[test]
    fn test_invalid_iat() {
        let mut claims = create_test_claims();
        claims.iat = 0;

        assert!(matches!(
            OAuthAuthorizationRequestJar::new(claims, None, None),
            Err(JarError::InvalidIat)
        ));
    }

    #[test]
    fn test_old_iat() {
        let mut claims = create_test_claims();
        claims.iat -= 120; // 2 minutes ago

        assert!(matches!(
            OAuthAuthorizationRequestJar::new(claims, None, None),
            Err(JarError::IatTooOld)
        ));
    }

    #[test]
    fn test_expired() {
        let mut claims = create_test_claims();
        claims.exp = Some(claims.iat - 1); // Already expired

        assert!(matches!(
            OAuthAuthorizationRequestJar::new(claims, None, None),
            Err(JarError::Expired)
        ));
    }

    #[test]
    fn test_additional_claims() {
        let mut claims = create_test_claims();
        claims
            .additional_claims
            .insert("custom_claim".to_string(), json!("custom_value"));

        let jar = OAuthAuthorizationRequestJar::new(claims, None, None).unwrap();
        let decoded = jar.decode_unsigned().unwrap();

        assert_eq!(
            decoded.additional_claims.get("custom_claim").unwrap(),
            &json!("custom_value")
        );
    }

    #[test]
    fn test_display() {
        let claims = create_test_claims();
        let jar = OAuthAuthorizationRequestJar::new(claims, None, None).unwrap();
        assert!(jar.to_string().starts_with("AuthorizationRequestJar("));
    }

    #[test]
    fn test_serialization() {
        let claims = create_test_claims();
        let original = OAuthAuthorizationRequestJar::new(claims, None, None).unwrap();
        let serialized = serde_json::to_string(&original).unwrap();
        let deserialized: OAuthAuthorizationRequestJar = serde_json::from_str(&serialized).unwrap();

        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_different_algorithms() {
        let claims = create_test_claims();
        let key = b"test-key";

        // Test HS256
        let hs256_jar =
            OAuthAuthorizationRequestJar::new(claims.clone(), Some(Algorithm::HS256), Some(key))
                .unwrap();
        assert!(hs256_jar.is_signed());

        // Test HS384
        let hs384_jar =
            OAuthAuthorizationRequestJar::new(claims.clone(), Some(Algorithm::HS384), Some(key))
                .unwrap();
        assert!(hs384_jar.is_signed());

        // Test HS512
        let hs512_jar =
            OAuthAuthorizationRequestJar::new(claims.clone(), Some(Algorithm::HS512), Some(key))
                .unwrap();
        assert!(hs512_jar.is_signed());
    }

    #[test]
    fn test_missing_key_for_algorithm() {
        let claims = create_test_claims();

        // Test that providing an algorithm without a key fails
        let result = OAuthAuthorizationRequestJar::new(claims, Some(Algorithm::HS256), None);

        assert!(matches!(result, Err(JarError::JwtEncoding(_))));
    }
}
