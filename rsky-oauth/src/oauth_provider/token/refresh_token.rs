use crate::oauth_provider::constants::{
    REFRESH_TOKEN_BYTES_LENGTH, REFRESH_TOKEN_PREFIX, TOKEN_ID_BYTES_LENGTH,
};
use rand::distr::Alphanumeric;
use rand::Rng;
use serde::de::Visitor;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

pub const REFRESH_TOKEN_LENGTH: usize = REFRESH_TOKEN_PREFIX.len() + REFRESH_TOKEN_BYTES_LENGTH; // hex encoding

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshToken(String);

impl RefreshToken {
    pub fn new(val: impl Into<String>) -> Result<Self, RefreshTokenError> {
        let val = val.into();

        if val.len() != REFRESH_TOKEN_LENGTH {
            return Err(RefreshTokenError::InvalidLength);
        }

        if !val.starts_with(REFRESH_TOKEN_PREFIX) {
            return Err(RefreshTokenError::InvalidLength);
        }

        Ok(RefreshToken(val))
    }

    pub fn val(&self) -> String {
        self.0.clone()
    }

    pub fn generate() -> RefreshToken {
        let token: String = rand::rng()
            .sample_iter(&Alphanumeric)
            .take(TOKEN_ID_BYTES_LENGTH)
            .map(char::from)
            .collect();
        let val = REFRESH_TOKEN_PREFIX.to_string() + token.as_str();
        RefreshToken::new(val).unwrap()
    }

    pub fn is_refresh_token(data: &str) -> bool {
        let prefix = &data.to_string()[..4];
        prefix == REFRESH_TOKEN_PREFIX
    }
}

impl Serialize for RefreshToken {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.0.as_str())
    }
}

// Custom visitor for deserialization
struct RefreshTokenVisitor;

impl Visitor<'_> for RefreshTokenVisitor {
    type Value = RefreshToken;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a string representing a RefreshToken")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        match RefreshToken::new(value) {
            Ok(uri) => Ok(uri),
            Err(error) => Err(E::custom(format!("{:?}", error))),
        }
    }
}

// Implement Deserialize using the visitor
impl<'de> Deserialize<'de> for RefreshToken {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(RefreshTokenVisitor)
    }
}

#[derive(Debug)]
pub enum RefreshTokenError {
    InvalidLength,
    InvalidFormat(String),
}
