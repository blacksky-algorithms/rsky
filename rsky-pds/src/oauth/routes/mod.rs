mod oauth_authorize;
mod oauth_authorize_accept;
mod oauth_authorize_reject;
mod oauth_authorize_sign_in;
mod oauth_introspect;
mod oauth_jwks;
mod oauth_par;
mod oauth_revoke;
mod oauth_token;
mod oauth_well_known;

use rocket::data::{FromData, ToByteUnit};
use rocket::futures::TryFutureExt;
use rocket::request::FromRequest;
use rocket::{routes, Request, Route};
use rsky_oauth::oauth_provider::account::account_store::SignInCredentials;
use rsky_oauth::oauth_provider::device::device_id::DeviceId;
use rsky_oauth::oauth_provider::request::request_uri::RequestUri;
use rsky_oauth::oauth_types::{
    OAuthAuthorizationRequestQuery, OAuthClientCredentials, OAuthClientId, OAuthRequestUri,
    OAuthTokenIdentification,
};

pub struct SignInPayload {
    csrf_token: String,
    request_uri: OAuthRequestUri,
    client_id: OAuthClientId,
    credentials: SignInCredentials,
}

pub struct AcceptQuery {
    pub csrf_token: String,
    pub request_uri: RequestUri,
    pub client_id: OAuthClientId,
    pub account_sub: String,
}

pub struct OAuthAcceptRequestBody {
    pub oauth_client_credentials: OAuthClientCredentials,
    pub oauth_token_identification: OAuthTokenIdentification,
}

pub struct OAuthRejectRequestBody {
    pub oauth_client_credentials: OAuthClientCredentials,
    pub oauth_token_identification: OAuthTokenIdentification,
}

pub struct OAuthSigninRequestBody {
    pub device_id: DeviceId,
    pub credentials: OAuthClientCredentials,
    pub authorization_request: OAuthAuthorizationRequestQuery,
}

pub struct DpopJkt(Option<String>);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for DpopJkt {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> rocket::request::Outcome<Self, Self::Error> {
        match req.headers().get_one("dpop") {
            None => rocket::request::Outcome::Success(DpopJkt(None)),
            Some(res) => rocket::request::Outcome::Success(DpopJkt(Some(res.to_string()))),
        }
    }
}

pub fn get_routes() -> Vec<Route> {
    routes![
        oauth_well_known::oauth_well_known,
        oauth_jwks::oauth_jwks,
        oauth_par::oauth_par,
        oauth_token::oauth_token,
        oauth_revoke::post_oauth_revoke,
        oauth_introspect::oauth_introspect,
        oauth_authorize::oauth_authorize,
        oauth_authorize_sign_in::oauth_authorize_sign_in,
        oauth_authorize_accept::oauth_authorize_accept,
        oauth_authorize_reject::oauth_authorize_reject
    ]
}
