use crate::oauth::routes::DpopJkt;
use crate::oauth::SharedOAuthProvider;
use http::header;
use rocket::data::{FromData, ToByteUnit};
use rocket::http::Status;
use rocket::serde::json::Json;
use rocket::{post, Data, Request, State};
use rsky_oauth::oauth_provider::errors::OAuthError;
use rsky_oauth::oauth_provider::OAuthTokenRequestBody;
use rsky_oauth::oauth_types::{OAuthClientCredentials, OAuthTokenRequest, OAuthTokenResponse};
use std::num::NonZeroU64;

#[post("/oauth/token", data = "<body>")]
pub async fn oauth_token(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    body: OAuthTokenRequestBody,
    dpop_jkt: DpopJkt,
) -> Result<Json<OAuthTokenResponse>, OAuthError> {
    unimplemented!()
    // let mut oauth_provider = shared_oauth_provider.oauth_provider.write().await;
    // let dpop_jkt = match dpop_jkt.0 {
    //     None => None,
    //     Some(res) => Some(res),
    // };
    // Ok(Json(
    //     oauth_provider
    //         .token(
    //             body.oauth_client_credentials,
    //             body.oauth_token_request,
    //             dpop_jkt,
    //         )
    //         .await?,
    // ))
}
