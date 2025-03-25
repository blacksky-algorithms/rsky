use crate::oauth_provider::client::client_auth::ClientAuth;
use crate::oauth_provider::client::client_id::ClientId;
use crate::oauth_provider::request::request_id::RequestId;
use crate::oauth_provider::request::request_uri::RequestUri;
use crate::oauth_types::{OAuthAuthorizationRequestParameters, OAuthRequestUri};

pub struct RequestInfo {
    pub id: RequestId,
    pub uri: RequestUri,
    pub parameters: OAuthAuthorizationRequestParameters,
    pub expires_at: u32,
    pub client_id: ClientId,
    pub client_auth: ClientAuth,
}
