use crate::oauth_provider::client::client_auth::ClientAuth;
use crate::oauth_provider::request::request_id::RequestId;
use crate::oauth_provider::request::request_uri::RequestUri;
use crate::oauth_types::{OAuthAuthorizationRequestParameters, OAuthClientId};
use chrono::{DateTime, Utc};

#[derive(PartialEq, Debug)]
pub struct RequestInfo {
    pub id: RequestId,
    pub uri: RequestUri,
    pub parameters: OAuthAuthorizationRequestParameters,
    pub expires_at: DateTime<Utc>,
    pub client_id: OAuthClientId,
    pub client_auth: ClientAuth,
}
