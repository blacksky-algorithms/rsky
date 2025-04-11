// Re-export components so they can be used from the top level of the jwk module
// mod alg;
mod alg;
mod errors;
mod jwk_base;
mod jwt;
mod jwt_decode;
mod jwt_verify;
mod key;
mod keyset;
mod util;

// Re-export public types and traits
pub use alg::*;
pub use errors::*;
pub use jwk_base::*;
pub use jwt::*;
pub use jwt_decode::*;
pub use jwt_verify::*;
pub use key::*;
pub use keyset::*;
pub use util::*;

// Re-export Zod's validation error as our own (this was in TypeScript original)
pub use serde::de::Error as ValidationError;

// Constants for MIME types used in the original TypeScript
pub const MIME_TYPE_CODE: &str = "application/vnd.ant.code";
pub const MIME_TYPE_MARKDOWN: &str = "text/markdown";
pub const MIME_TYPE_HTML: &str = "text/html";
pub const MIME_TYPE_SVG: &str = "image/svg+xml";
pub const MIME_TYPE_MERMAID: &str = "application/vnd.ant.mermaid";
pub const MIME_TYPE_REACT: &str = "application/vnd.ant.react";

// Common traits that might be needed throughout the module
pub trait JsonWebKey {
    /// Get the key ID if present
    fn kid(&self) -> Option<&str>;

    /// Get the key type
    fn kty(&self) -> &str;

    /// Get the algorithm if specified
    fn alg(&self) -> Option<&str>;

    /// Whether this is a private key
    fn is_private(&self) -> bool;
}

pub trait JsonWebToken {
    /// Get the token's header claims
    fn header(&self) -> &jwt::JwtHeader;

    /// Get the token's payload claims
    fn payload(&self) -> &jwt::JwtPayload;

    /// Return the encoded token string
    fn to_string(&self) -> String;
}

// Helper types that might be useful throughout the module
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TokenValidation {
    pub required_claims: Vec<String>,
    pub allowed_issuers: Option<Vec<String>>,
    pub allowed_audiences: Option<Vec<String>>,
    pub max_age: Option<u64>,
    pub clock_tolerance: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenMetadata {
    pub issuer: Option<String>,
    pub subject: Option<String>,
    pub audience: Option<Vec<String>>,
    pub expiration: Option<u64>,
    pub not_before: Option<u64>,
    pub issued_at: Option<u64>,
    pub jwt_id: Option<String>,
}

// Helper functions that might be useful throughout the module
pub fn is_token_expired(exp: Option<u64>, clock_tolerance: Option<u64>) -> bool {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    if let Some(exp) = exp {
        let tolerance = clock_tolerance.unwrap_or(0);
        exp.saturating_add(tolerance) < now
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_validation_default() {
        let validation = TokenValidation::default();
        assert!(validation.required_claims.is_empty());
        assert!(validation.allowed_issuers.is_none());
        assert!(validation.allowed_audiences.is_none());
        assert!(validation.max_age.is_none());
        assert!(validation.clock_tolerance.is_none());
    }

    #[test]
    fn test_is_token_expired() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Test expired token
        assert!(is_token_expired(Some(now - 100), None));

        // Test valid token
        assert!(!is_token_expired(Some(now + 100), None));

        // Test with clock tolerance
        assert!(!is_token_expired(Some(now - 50), Some(100)));

        // Test None expiration
        assert!(!is_token_expired(None, None));
    }
}
