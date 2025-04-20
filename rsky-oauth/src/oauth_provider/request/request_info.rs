use crate::oauth_provider::client::client_auth::ClientAuth;
use crate::oauth_provider::request::request_id::RequestId;
use crate::oauth_provider::request::request_uri::RequestUri;
use crate::oauth_types::{OAuthAuthorizationRequestParameters, OAuthClientId};
use serde::{Deserialize, Serialize};

#[derive(PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct RequestInfo {
    pub id: RequestId,
    pub uri: RequestUri,
    pub parameters: OAuthAuthorizationRequestParameters,
    pub expires_at: i64,
    pub client_id: OAuthClientId,
    pub client_auth: ClientAuth,
}
