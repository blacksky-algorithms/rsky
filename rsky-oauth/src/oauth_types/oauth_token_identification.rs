//! OAuth token identification types and validation.

use serde::{Serialize, Deserialize};
use std::fmt;
use std::str::FromStr;
use urlencoding::decode;

use crate::oauth_types::{OAuthAccessToken, AccessTokenError, OAuthRefreshToken, RefreshTokenError};

/// Token type hint values for token identification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenTypeHint {
    /// Access token
    AccessToken,
    /// Refresh token
    RefreshToken,
}

impl TokenTypeHint {
    /// Get all supported token type hints
    pub fn variants() -> &'static [TokenTypeHint] {
        &[TokenTypeHint::AccessToken, TokenTypeHint::RefreshToken]
    }
}

impl fmt::Display for TokenTypeHint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenTypeHint::AccessToken => write!(f, "access_token"),
            TokenTypeHint::RefreshToken => write!(f, "refresh_token"),
        }
    }
}

impl FromStr for TokenTypeHint {
    type Err = TokenIdentificationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "access_token" => Ok(TokenTypeHint::AccessToken),
            "refresh_token" => Ok(TokenTypeHint::RefreshToken),
            _ => Err(TokenIdentificationError::InvalidTokenTypeHint(s.to_string())),
        }
    }
}

impl AsRef<str> for TokenTypeHint {
    fn as_ref(&self) -> &str {
        match self {
            TokenTypeHint::AccessToken => "access_token",
            TokenTypeHint::RefreshToken => "refresh_token",
        }
    }
}

/// A token to be identified, which could be either an access token or a refresh token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// Access token
    AccessToken(OAuthAccessToken),
    /// Refresh token
    RefreshToken(OAuthRefreshToken),
}

impl Token {
    /// Create a new Token from a string, trying to identify it as an access token or refresh token.
    ///
    /// Since both token types are just validated strings, this will always succeed
    /// if the token string is not empty. The token_type_hint can be used to
    /// indicate which token type to try first.
    pub fn new(
        token: impl Into<String>,
        token_type_hint: Option<TokenTypeHint>,
    ) -> Result<Self, TokenIdentificationError> {
        let token_str = token.into();
        
        if token_str.is_empty() {
            return Err(TokenIdentificationError::EmptyToken);
        }
        
        // First try the hinted token type if provided
        if let Some(hint) = token_type_hint {
            match hint {
                TokenTypeHint::AccessToken => {
                    if let Ok(access_token) = OAuthAccessToken::new(&token_str) {
                        return Ok(Token::AccessToken(access_token));
                    }
                },
                TokenTypeHint::RefreshToken => {
                    if let Ok(refresh_token) = OAuthRefreshToken::new(&token_str) {
                        return Ok(Token::RefreshToken(refresh_token));
                    }
                },
            }
        }
        
        // Then try both types
        if let Ok(access_token) = OAuthAccessToken::new(&token_str) {
            Ok(Token::AccessToken(access_token))
        } else if let Ok(refresh_token) = OAuthRefreshToken::new(&token_str) {
            Ok(Token::RefreshToken(refresh_token))
        } else {
            // This should never happen since both token types only check for emptiness
            Err(TokenIdentificationError::EmptyToken)
        }
    }
    
    /// Return this token's value as a string.
    pub fn token_value(&self) -> &str {
        match self {
            Token::AccessToken(token) => token.as_ref(),
            Token::RefreshToken(token) => token.as_ref(),
        }
    }
    
    /// Determine if this is an access token.
    pub fn is_access_token(&self) -> bool {
        matches!(self, Token::AccessToken(_))
    }
    
    /// Determine if this is a refresh token.
    pub fn is_refresh_token(&self) -> bool {
        matches!(self, Token::RefreshToken(_))
    }
    
    /// Extract the access token if this is an access token.
    pub fn as_access_token(&self) -> Option<&OAuthAccessToken> {
        match self {
            Token::AccessToken(token) => Some(token),
            _ => None,
        }
    }
    
    /// Extract the refresh token if this is a refresh token.
    pub fn as_refresh_token(&self) -> Option<&OAuthRefreshToken> {
        match self {
            Token::RefreshToken(token) => Some(token),
            _ => None,
        }
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::AccessToken(token) => write!(f, "{}", token),
            Token::RefreshToken(token) => write!(f, "{}", token),
        }
    }
}

/// An OAuth token identification object, for use with token introspection and revocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthTokenIdentification {
    /// The token to identify
    pub token: String,
    
    /// A hint about the type of token
    #[serde(rename = "token_type_hint", skip_serializing_if = "Option::is_none")]
    pub token_type_hint: Option<TokenTypeHint>,
}

impl OAuthTokenIdentification {
    /// Create a new token identification.
    pub fn new(
        token: impl Into<String>,
        token_type_hint: Option<TokenTypeHint>,
    ) -> Result<Self, TokenIdentificationError> {
        let token = token.into();
        if token.is_empty() {
            return Err(TokenIdentificationError::EmptyToken);
        }
        
        Ok(Self {
            token,
            token_type_hint,
        })
    }
    
    /// Convert this token identification into a parsed token.
    pub fn into_token(self) -> Result<Token, TokenIdentificationError> {
        Token::new(self.token, self.token_type_hint)
    }
    
    /// Parse from form-encoded body.
    pub fn from_form(form: &str) -> Result<Self, TokenIdentificationError> {
        let mut token = None;
        let mut token_type_hint = None;
        
        for pair in form.split('&') {
            let mut parts = pair.split('=');
            if let (Some(key), Some(value)) = (parts.next(), parts.next()) {
                let key = decode(key)
                    .map_err(|_| TokenIdentificationError::InvalidFormEncoding)?;
                let value = decode(value)
                    .map_err(|_| TokenIdentificationError::InvalidFormEncoding)?;
                
                match key.as_ref() {
                    "token" => token = Some(value.into_owned()),
                    "token_type_hint" => {
                        token_type_hint = Some(
                            value.parse()
                                .map_err(|_| TokenIdentificationError::InvalidTokenTypeHint(value.into_owned()))?
                        );
                    },
                    _ => {} // Ignore unknown parameters
                }
            }
        }
        
        let token = token.ok_or(TokenIdentificationError::MissingToken)?;
        
        Self::new(token, token_type_hint)
    }
    
    /// Convert to form-encoded body.
    pub fn to_form(&self) -> String {
        let mut parts = Vec::new();
        
        parts.push(format!("token={}", urlencoding::encode(&self.token)));
        
        if let Some(hint) = &self.token_type_hint {
            parts.push(format!("token_type_hint={}", urlencoding::encode(hint.as_ref())));
        }
        
        parts.join("&")
    }
}

/// Errors that can occur when working with token identification.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TokenIdentificationError {
    #[error("Token cannot be empty")]
    EmptyToken,
    
    #[error("Invalid token type hint: {0}")]
    InvalidTokenTypeHint(String),
    
    #[error("Missing required parameter 'token'")]
    MissingToken,
    
    #[error("Invalid form encoding")]
    InvalidFormEncoding,
    
    #[error("Access token error: {0}")]
    AccessTokenError(#[from] AccessTokenError),
    
    #[error("Refresh token error: {0}")]
    RefreshTokenError(#[from] RefreshTokenError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_type_hint_variants() {
        let variants = TokenTypeHint::variants();
        assert_eq!(variants.len(), 2);
        assert!(variants.contains(&TokenTypeHint::AccessToken));
        assert!(variants.contains(&TokenTypeHint::RefreshToken));
    }

    #[test]
    fn test_token_type_hint_display() {
        assert_eq!(TokenTypeHint::AccessToken.to_string(), "access_token");
        assert_eq!(TokenTypeHint::RefreshToken.to_string(), "refresh_token");
    }

    #[test]
    fn test_token_type_hint_from_str() {
        assert_eq!("access_token".parse::<TokenTypeHint>().unwrap(), TokenTypeHint::AccessToken);
        assert_eq!("refresh_token".parse::<TokenTypeHint>().unwrap(), TokenTypeHint::RefreshToken);
        
        assert!("invalid".parse::<TokenTypeHint>().is_err());
    }

    #[test]
    fn test_token_creation() {
        let access_token = Token::new("example_token", Some(TokenTypeHint::AccessToken)).unwrap();
        assert!(access_token.is_access_token());
        assert!(!access_token.is_refresh_token());
        
        let refresh_token = Token::new("example_token", Some(TokenTypeHint::RefreshToken)).unwrap();
        assert!(!refresh_token.is_access_token());
        assert!(refresh_token.is_refresh_token());
        
        // Without hint
        let any_token = Token::new("example_token", None).unwrap();
        assert!(any_token.is_access_token() || any_token.is_refresh_token());
        assert_eq!(any_token.token_value(), "example_token");
        
        // Empty token
        assert!(matches!(
            Token::new("", None),
            Err(TokenIdentificationError::EmptyToken)
        ));
    }

    #[test]
    fn test_token_accessors() {
        let access_token = Token::new("example_token", Some(TokenTypeHint::AccessToken)).unwrap();
        assert!(access_token.as_access_token().is_some());
        assert!(access_token.as_refresh_token().is_none());
        
        let refresh_token = Token::new("example_token", Some(TokenTypeHint::RefreshToken)).unwrap();
        assert!(refresh_token.as_access_token().is_none());
        assert!(refresh_token.as_refresh_token().is_some());
    }

    #[test]
    fn test_token_display() {
        let token = Token::new("example_token", None).unwrap();
        assert_eq!(token.to_string(), "example_token");
    }

    #[test]
    fn test_token_identification_new() {
        let id = OAuthTokenIdentification::new("example_token", Some(TokenTypeHint::AccessToken)).unwrap();
        assert_eq!(id.token, "example_token");
        assert_eq!(id.token_type_hint, Some(TokenTypeHint::AccessToken));
        
        let id = OAuthTokenIdentification::new("example_token", None).unwrap();
        assert_eq!(id.token, "example_token");
        assert_eq!(id.token_type_hint, None);
        
        assert!(matches!(
            OAuthTokenIdentification::new("", None),
            Err(TokenIdentificationError::EmptyToken)
        ));
    }

    #[test]
    fn test_token_identification_into_token() {
        let id = OAuthTokenIdentification::new("example_token", Some(TokenTypeHint::AccessToken)).unwrap();
        let token = id.into_token().unwrap();
        assert!(token.is_access_token());
        assert_eq!(token.token_value(), "example_token");
    }

    #[test]
    fn test_token_identification_from_form() {
        let form = "token=example_token&token_type_hint=access_token";
        let id = OAuthTokenIdentification::from_form(form).unwrap();
        assert_eq!(id.token, "example_token");
        assert_eq!(id.token_type_hint, Some(TokenTypeHint::AccessToken));
        
        // URL encoded form
        let form = "token=example%20token&token_type_hint=refresh_token";
        let id = OAuthTokenIdentification::from_form(form).unwrap();
        assert_eq!(id.token, "example token");
        assert_eq!(id.token_type_hint, Some(TokenTypeHint::RefreshToken));
        
        // Missing token
        let form = "token_type_hint=access_token";
        assert!(matches!(
            OAuthTokenIdentification::from_form(form),
            Err(TokenIdentificationError::MissingToken)
        ));
        
        // Invalid token type hint
        let form = "token=example_token&token_type_hint=invalid";
        assert!(matches!(
            OAuthTokenIdentification::from_form(form),
            Err(TokenIdentificationError::InvalidTokenTypeHint(_))
        ));
        
        // Invalid form encoding
        let form = "token=%invalid";
        assert!(matches!(
            OAuthTokenIdentification::from_form(form),
            Err(TokenIdentificationError::InvalidFormEncoding)
        ));
    }

    #[test]
    fn test_token_identification_to_form() {
        let id = OAuthTokenIdentification::new("example token", Some(TokenTypeHint::AccessToken)).unwrap();
        let form = id.to_form();
        assert_eq!(form, "token=example%20token&token_type_hint=access_token");
        
        let id = OAuthTokenIdentification::new("example_token", None).unwrap();
        let form = id.to_form();
        assert_eq!(form, "token=example_token");
    }

    #[test]
    fn test_serialization() {
        let id = OAuthTokenIdentification::new("example_token", Some(TokenTypeHint::AccessToken)).unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, r#"{"token":"example_token","token_type_hint":"access_token"}"#);
        
        let id = OAuthTokenIdentification::new("example_token", None).unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, r#"{"token":"example_token"}"#);
        
        // Deserialization
        let id: OAuthTokenIdentification = serde_json::from_str(r#"{"token":"example_token","token_type_hint":"refresh_token"}"#).unwrap();
        assert_eq!(id.token, "example_token");
        assert_eq!(id.token_type_hint, Some(TokenTypeHint::RefreshToken));
    }
}