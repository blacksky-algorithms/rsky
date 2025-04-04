use crate::oauth::SharedOAuthProvider;
use http::header;
use rocket::data::{FromData, ToByteUnit};
use rocket::http::Status;
use rocket::request::FromRequest;
use rocket::{get, post, Data, Request, State};
use rsky_oauth::oauth_provider::errors::OAuthError;
use rsky_oauth::oauth_provider::{OAuthRevokeGetRequestBody, OAuthRevokeRequestBody};
use rsky_oauth::oauth_types::{OAuthTokenIdentification, TokenTypeHint};
use std::num::NonZeroU64;

#[get("/oauth/revoke")]
pub async fn get_oauth_revoke(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    body: OAuthRevokeGetRequestBody,
) -> Result<(), OAuthError> {
    unimplemented!()
    // let mut oauth_provider = shared_oauth_provider.oauth_provider.write().await;
    // match oauth_provider
    //     .revoke(&body.oauth_token_identification)
    //     .await
    // {
    //     Ok(res) => Ok(()),
    //     Err(e) => Err(e),
    // }
}

#[post("/oauth/revoke", data = "<body>")]
pub async fn post_oauth_revoke(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    body: OAuthRevokeRequestBody,
) -> Result<(), OAuthError> {
    unimplemented!()
    // let mut creator = shared_oauth_provider.oauth_provider.write().await;
    // let x = creator(d);
    // match oauth_provider
    //     .revoke(&body.oauth_token_identification)
    //     .await
    // {
    //     Ok(res) => Ok(()),
    //     Err(e) => Err(e),
    // }
}
