mod client_metadata;
mod metadata;
mod request;
mod response;
mod token;

use crate::vendored::atrium_oauth_client::atproto::{KnownScope, Scope};
pub use client_metadata::{OAuthClientMetadata, TryIntoOAuthClientMetadata};
pub use metadata::{OAuthAuthorizationServerMetadata, OAuthProtectedResourceMetadata};
pub use request::{
    AuthorizationCodeChallengeMethod, AuthorizationResponseType,
    PushedAuthorizationRequestParameters, RefreshRequestParameters, TokenGrantType,
    TokenRequestParameters,
};
pub use response::{OAuthPusehedAuthorizationRequestResponse, OAuthTokenResponse};
use serde::Deserialize;
pub use token::TokenSet;

#[derive(Debug, Deserialize)]
pub enum AuthorizeOptionPrompt {
    Login,
    None,
    Consent,
    SelectAccount,
}

impl From<AuthorizeOptionPrompt> for String {
    fn from(value: AuthorizeOptionPrompt) -> Self {
        match value {
            AuthorizeOptionPrompt::Login => String::from("login"),
            AuthorizeOptionPrompt::None => String::from("none"),
            AuthorizeOptionPrompt::Consent => String::from("consent"),
            AuthorizeOptionPrompt::SelectAccount => String::from("select_account"),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AuthorizeOptions {
    pub redirect_uri: Option<String>,
    pub scopes: Vec<Scope>,
    pub prompt: Option<AuthorizeOptionPrompt>,
    pub state: Option<String>,
}

impl Default for AuthorizeOptions {
    fn default() -> Self {
        Self {
            redirect_uri: None,
            scopes: vec![Scope::Known(KnownScope::Atproto)],
            prompt: None,
            state: None,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CallbackParams {
    pub code: String,
    pub state: Option<String>,
    pub iss: Option<String>,
}
