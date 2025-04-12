use crate::oauth_provider::constants::{SESSION_ID_BYTES_LENGTH, SESSION_ID_PREFIX};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

const SESSION_ID_LENGTH: usize = SESSION_ID_PREFIX.len() + SESSION_ID_BYTES_LENGTH * 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionId(String);

impl SessionId {
    pub fn new(data: impl Into<String>) -> Result<Self, SessionIdError> {
        let data = data.into();

        if data.len() != SESSION_ID_LENGTH {
            return Err(SessionIdError::Invalid);
        }

        if !data.starts_with(SESSION_ID_PREFIX) {
            return Err(SessionIdError::Invalid);
        }

        Ok(Self(data))
    }

    /// Get the underlying issuer URL string.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl AsRef<str> for SessionId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for SessionId {
    type Err = SessionIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

/// Errors that can occur when creating an OAuthIssuerIdentifier.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SessionIdError {
    #[error("Invalid format")]
    Invalid,
}

//TODO generate hex id
pub async fn generate_session_id() -> SessionId {
    SessionId("test".to_string())
}
