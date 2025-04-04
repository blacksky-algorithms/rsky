use crate::oauth_types::{OAuthAccessToken, OAuthTokenType};

pub struct AuthorizationHeader {
    pub token_type: OAuthTokenType,
    pub oauth_access_token: OAuthAccessToken,
}

impl AuthorizationHeader {
    pub fn new(header: impl Into<String>) -> Result<Self, AuthorizationHeaderError> {
        let header = header.into();
        if header.is_empty() {
            return Err(AuthorizationHeaderError::Empty);
        }

        let mut parts = header.split(" ");
        let token_type = match parts.next() {
            None => {
                return Err(AuthorizationHeaderError::Invalid);
            }
            Some(token_type) => {
                if token_type == "DPoP" {
                    OAuthTokenType::DPoP
                } else if token_type == "Bearer" {
                    OAuthTokenType::Bearer
                } else {
                    return Err(AuthorizationHeaderError::Invalid);
                }
            }
        };
        let oauth_access_token = match parts.next() {
            None => {
                return Err(AuthorizationHeaderError::Invalid);
            }
            Some(oauth_access_token) => match OAuthAccessToken::new(oauth_access_token) {
                Ok(oauth_access_token) => oauth_access_token,
                Err(e) => {
                    return Err(AuthorizationHeaderError::Invalid);
                }
            },
        };
        Ok(Self {
            token_type,
            oauth_access_token,
        })
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AuthorizationHeaderError {
    #[error("Refresh token cannot be empty")]
    Empty,
    #[error("Refresh token is invalid")]
    Invalid,
}
