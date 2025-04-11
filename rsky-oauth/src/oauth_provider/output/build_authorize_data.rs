use crate::oauth_provider::account::account::Account;
use crate::oauth_provider::client::client::Client;
use crate::oauth_provider::oauth_provider::OAuthProviderSession;
use crate::oauth_provider::request::request_uri::RequestUri;
use crate::oauth_types::{OAuthAuthorizationRequestParameters, OAuthIssuerIdentifier};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeDetail {
    scope: String,
    description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizationResultAuthorize {
    pub issuer: OAuthIssuerIdentifier,
    pub client: Client,
    pub parameters: OAuthAuthorizationRequestParameters,
    pub authorize: Authorize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Authorize {
    pub uri: RequestUri,
    pub scope_details: Option<Vec<ScopeDetail>>,
    pub sessions: Vec<Session>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub account: Account,
    pub selected: bool,
    pub login_required: bool,
    pub consent_required: bool,
}

impl From<OAuthProviderSession> for Session {
    fn from(value: OAuthProviderSession) -> Self {
        Session {
            account: value.account,
            selected: value.selected,
            login_required: value.login_required,
            consent_required: value.consent_required,
        }
    }
}
