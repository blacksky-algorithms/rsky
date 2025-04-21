use crate::oauth_provider::client::client_auth::ClientAuth;
use crate::oauth_provider::device::device_id::DeviceId;
use crate::oauth_provider::oidc::sub::Sub;
use crate::oauth_provider::request::code::Code;
use crate::oauth_types::{OAuthAuthorizationRequestParameters, OAuthClientId};
use chrono::{DateTime, Utc};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestData {
    pub client_id: OAuthClientId,
    pub client_auth: ClientAuth,
    pub parameters: OAuthAuthorizationRequestParameters,
    pub expires_at: DateTime<Utc>,
    pub device_id: Option<DeviceId>,
    pub sub: Option<Sub>,
    pub code: Option<Code>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestDataAuthorized {
    pub client_id: OAuthClientId,
    pub client_auth: ClientAuth,
    pub parameters: OAuthAuthorizationRequestParameters,
    pub expires_at: DateTime<Utc>,
    pub device_id: DeviceId,
    pub sub: Sub,
    pub code: Option<Code>,
}

impl RequestDataAuthorized {
    pub fn new(data: RequestData) -> Result<Self, RequestDataAuthorizedError> {
        let device_id = match data.device_id {
            None => {
                return Err(RequestDataAuthorizedError::Invalid);
            }
            Some(device_id) => device_id,
        };
        let sub = match data.sub {
            None => {
                return Err(RequestDataAuthorizedError::Invalid);
            }
            Some(sub) => sub,
        };
        Ok(Self {
            client_id: data.client_id,
            client_auth: data.client_auth,
            parameters: data.parameters,
            expires_at: data.expires_at,
            device_id,
            sub,
            code: data.code,
        })
    }
}

/// Errors that can occur when creating an OAuthRefreshToken.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RequestDataAuthorizedError {
    #[error("Refresh token cannot be empty")]
    Invalid,
}
