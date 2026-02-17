//! Service authentication handling
//!
//! Validates service auth tokens from AT Protocol clients.
//! For MVP, we do basic JWT validation. Full validation would verify
//! the signature against the PDS's signing key.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::error::{Error, Result};

/// Decoded service auth token claims
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServiceAuthClaims {
    /// Issuer (the user's DID when PDS signs on user's behalf)
    pub iss: String,
    /// Audience (should be the video service DID)
    pub aud: String,
    /// Subject (the user's DID) - optional, may use iss instead
    pub sub: Option<String>,
    /// Lexicon method being authorized
    pub lxm: Option<String>,
    /// Expiration time (Unix timestamp)
    pub exp: i64,
    /// Issued at time (Unix timestamp)
    pub iat: Option<i64>,
    /// JWT ID
    pub jti: Option<String>,
}

impl ServiceAuthClaims {
    /// Get the user DID - uses sub if present, otherwise iss
    pub fn user_did(&self) -> &str {
        self.sub.as_deref().unwrap_or(&self.iss)
    }
}

/// Extract and validate the Authorization header
pub fn extract_auth_header(auth_header: Option<&str>) -> Result<String> {
    let header = auth_header
        .ok_or_else(|| Error::Unauthorized("Missing Authorization header".to_string()))?;

    if !header.starts_with("Bearer ") {
        return Err(Error::Unauthorized(
            "Invalid Authorization header format".to_string(),
        ));
    }

    Ok(header[7..].to_string())
}

/// Decode and validate a service auth JWT (basic validation)
///
/// For MVP, this does:
/// - Decode the JWT payload
/// - Check expiration
/// - Optionally validate audience matches expected DID
///
/// Full implementation would also:
/// - Resolve the issuer's signing key from their PDS
/// - Verify the JWT signature
pub fn validate_service_auth(
    token: &str,
    expected_aud: &str,
    expected_lxm: Option<&str>,
) -> Result<ServiceAuthClaims> {
    // Split JWT into parts
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(Error::Unauthorized("Invalid JWT format".to_string()));
    }

    // Decode the payload (middle part)
    let payload_bytes = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|e| Error::Unauthorized(format!("Failed to decode JWT payload: {}", e)))?;

    let claims: ServiceAuthClaims = serde_json::from_slice(&payload_bytes)
        .map_err(|e| Error::Unauthorized(format!("Failed to parse JWT claims: {}", e)))?;

    debug!(
        "Service auth: iss={}, sub={:?}, aud={}, lxm={:?}, user_did={}",
        claims.iss,
        claims.sub,
        claims.aud,
        claims.lxm,
        claims.user_did()
    );

    // Check expiration
    let now = chrono::Utc::now().timestamp();
    if claims.exp < now {
        return Err(Error::Unauthorized("Token has expired".to_string()));
    }

    // Validate audience
    if claims.aud != expected_aud {
        warn!(
            "Invalid audience: expected {}, got {}",
            expected_aud, claims.aud
        );
        return Err(Error::Unauthorized("Invalid token audience".to_string()));
    }

    // Validate lexicon method if expected
    if let Some(expected) = expected_lxm {
        if claims.lxm.as_deref() != Some(expected) {
            warn!("Invalid lxm: expected {}, got {:?}", expected, claims.lxm);
            return Err(Error::Unauthorized("Invalid token scope".to_string()));
        }
    }

    Ok(claims)
}

/// Decode service auth JWT without audience validation
/// Used for uploadVideo where the token's audience is the user's PDS DID,
/// not the video service DID. The video service forwards this token to the PDS.
pub fn decode_service_auth(token: &str) -> Result<ServiceAuthClaims> {
    // Split JWT into parts
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(Error::Unauthorized("Invalid JWT format".to_string()));
    }

    // Decode the payload (middle part)
    let payload_bytes = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|e| Error::Unauthorized(format!("Failed to decode JWT payload: {}", e)))?;

    let claims: ServiceAuthClaims = serde_json::from_slice(&payload_bytes)
        .map_err(|e| Error::Unauthorized(format!("Failed to parse JWT claims: {}", e)))?;

    debug!(
        "Service auth (no aud check): iss={}, sub={:?}, aud={}, lxm={:?}, user_did={}",
        claims.iss,
        claims.sub,
        claims.aud,
        claims.lxm,
        claims.user_did()
    );

    // Check expiration
    let now = chrono::Utc::now().timestamp();
    if claims.exp < now {
        return Err(Error::Unauthorized("Token has expired".to_string()));
    }

    Ok(claims)
}

/// Extract the user DID from an Authorization header
pub fn get_user_did(auth_header: Option<&str>, service_did: &str) -> Result<String> {
    let token = extract_auth_header(auth_header)?;
    let claims = validate_service_auth(&token, service_did, None)?;
    Ok(claims.user_did().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_auth_header() {
        assert!(extract_auth_header(None).is_err());
        assert!(extract_auth_header(Some("Basic xyz")).is_err());
        assert_eq!(
            extract_auth_header(Some("Bearer mytoken")).unwrap(),
            "mytoken"
        );
    }
}
