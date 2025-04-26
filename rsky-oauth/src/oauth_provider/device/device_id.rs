use crate::oauth_provider::constants::{DEVICE_ID_BYTES_LENGTH, DEVICE_ID_PREFIX};
use rand::distr::Alphanumeric;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

const DEVICE_ID_LENGTH: usize = DEVICE_ID_PREFIX.len() + DEVICE_ID_BYTES_LENGTH;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceId(String);

impl DeviceId {
    pub fn new(data: impl Into<String>) -> Result<Self, DeviceIdError> {
        let data = data.into();

        if data.len() != DEVICE_ID_LENGTH {
            return Err(DeviceIdError::Invalid);
        }

        if !data.starts_with(DEVICE_ID_PREFIX) {
            return Err(DeviceIdError::Invalid);
        }

        Ok(Self(data))
    }

    /// Get the underlying issuer URL string.
    pub fn into_inner(self) -> String {
        self.0
    }

    pub fn generate() -> DeviceId {
        let token: String = rand::rng()
            .sample_iter(&Alphanumeric)
            .take(DEVICE_ID_BYTES_LENGTH)
            .map(char::from)
            .collect();
        let val = DEVICE_ID_PREFIX.to_string() + token.as_str();
        DeviceId::new(val).unwrap()
    }
}

impl AsRef<str> for DeviceId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for DeviceId {
    type Err = DeviceIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

/// Errors that can occur when creating an OAuthIssuerIdentifier.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DeviceIdError {
    #[error("Invalid format")]
    Invalid,
}
