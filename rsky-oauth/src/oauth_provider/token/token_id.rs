use crate::oauth_provider::constants::{TOKEN_ID_BYTES_LENGTH, TOKEN_ID_PREFIX};
use crate::oauth_provider::oidc::sub::Sub;
use rand::distr::Alphanumeric;
use rand::Rng;
use serde::de::Visitor;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

const TOKEN_ID_LENGTH: usize = TOKEN_ID_PREFIX.len() + TOKEN_ID_BYTES_LENGTH;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TokenId(String);

/// Errors that can occur when working with token identification.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TokenIdError {
    #[error("Invalid Length")]
    InvalidLength,
    #[error("Invalid token ID format")]
    InvalidFormat,
}

impl TokenId {
    pub fn new(token: impl Into<String>) -> Result<Self, TokenIdError> {
        let token = token.into();
        if token.len() != TOKEN_ID_LENGTH {
            return Err(TokenIdError::InvalidLength);
        }

        if !token.starts_with(TOKEN_ID_PREFIX) {
            return Err(TokenIdError::InvalidFormat);
        }

        Ok(Self(token))
    }

    pub fn val(&self) -> String {
        self.0.clone()
    }

    /// Get the underlying issuer URL string.
    pub fn into_inner(self) -> String {
        self.0
    }

    pub fn generate() -> TokenId {
        let token: String = rand::rng()
            .sample_iter(&Alphanumeric)
            .take(TOKEN_ID_BYTES_LENGTH)
            .map(char::from)
            .collect();
        let val = TOKEN_ID_PREFIX.to_string() + token.as_str();
        TokenId::new(val).unwrap()
    }
}

impl Serialize for TokenId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.0.as_str())
    }
}

// Custom visitor for deserialization
struct TokenIdVisitor;

impl Visitor<'_> for TokenIdVisitor {
    type Value = TokenId;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a string representing a TokenId")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        match TokenId::new(value) {
            Ok(uri) => Ok(uri),
            Err(error) => Err(E::custom(format!("{:?}", error))),
        }
    }
}

// Implement Deserialize using the visitor
impl<'de> Deserialize<'de> for TokenId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(TokenIdVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_id() {
        let token_id = TokenId::new("tok-dwadwdaddwadwdad").unwrap();
        assert_eq!(token_id.into_inner(), "tok-dwadwdaddwadwdad");
        let token_id = TokenId::generate();
        let val = token_id.into_inner();
        TokenId::new(val).unwrap();

        let invalid_format_token_id = TokenId::new("aaaadwadwdaddwadwdad").unwrap_err();
        assert_eq!(invalid_format_token_id, TokenIdError::InvalidFormat);

        let invalid_length = TokenId::new("tok-dwadwda").unwrap_err();
        assert_eq!(invalid_length, TokenIdError::InvalidLength);
    }
}
