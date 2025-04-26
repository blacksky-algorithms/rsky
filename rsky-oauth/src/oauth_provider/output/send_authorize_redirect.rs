// https://datatracker.ietf.org/doc/html/draft-ietf-oauth-v2-1-11#section-7.5.4
const REDIRECT_STATUS_CODE: u64 = 303;
//
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizationResponseParameters {
    // Will be added from AuthorizationResultRedirect['issuer']
    // iss: string // rfc9207

    // Will be added from AuthorizationResultRedirect['parameters']
    // state?: string
    pub code: Option<Code>,
    pub id_token: Option<String>,
    pub access_token: Option<String>,
    pub token_type: Option<OAuthTokenType>,
    pub expires_in: Option<String>,

    pub response: Option<String>,      // FAPI JARM
    pub session_state: Option<String>, // OIDC Session Management

    pub error: Option<String>,
    pub error_description: Option<String>,
    pub error_uri: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizationResultRedirect {
    pub issuer: OAuthIssuerIdentifier,
    pub parameters: OAuthAuthorizationRequestParameters,
    pub redirect: AuthorizationResponseParameters,
}

impl<'r> Responder<'r, 'static> for AuthorizationResultRedirect {
    fn respond_to(self, _: &'r Request<'_>) -> response::Result<'static> {
        let mut response = Response::build();
        let issuer = self.issuer;
        let parameters = self.parameters;
        let redirect = self.redirect;
        let uri = match parameters.redirect_uri {
            None => {
                unimplemented!()
            }
            Some(uri) => uri,
        };

        let mode = parameters.response_mode.unwrap_or(ResponseMode::Query); // @TODO: default should depend on response_type

        // serialize struct into json string
        response.raw_header("Cache-Control", "no-store");

        match mode {
            ResponseMode::Query => {
                let mut url = Url::parse(uri.as_str()).unwrap();
                //TODO
                // url.query_pairs_mut().clear().append_pair();
                response.status(Status::SeeOther);
                response.raw_header("Location", url.to_string());
                response.ok()
            }
            ResponseMode::Fragment => {
                let mut url = Url::parse(uri.as_str()).unwrap();
                unimplemented!()
            }
            ResponseMode::FormPost => {
                unimplemented!()
            }
        }
    }
}

use crate::oauth_provider::output::build_authorize_data::AuthorizationResultAuthorize;
use crate::oauth_provider::request::code::Code;
use crate::oauth_types::{
    OAuthAuthorizationRequestParameters, OAuthIssuerIdentifier, OAuthTokenType, ResponseMode,
};
use rocket::http::Status;
use rocket::response::Responder;
use rocket::{response, Request, Response};
use serde::{Deserialize, Serialize};
use url::Url;

/// Authorization Result
///
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthorizationResult {
    Redirect(AuthorizationResultRedirect),
    Authorize(AuthorizationResultAuthorize),
}
