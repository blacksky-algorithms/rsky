use crate::oauth_provider::client::client::Client;
use crate::oauth_types::OAuthAuthorizationRequestParameters;

pub struct ScopeDetail {
    scope: String,
    description: Option<String>,
}

pub struct AuthorizationResultAuthorize {
    issuer: String,
    client: Client,
    parameters: OAuthAuthorizationRequestParameters,
}
