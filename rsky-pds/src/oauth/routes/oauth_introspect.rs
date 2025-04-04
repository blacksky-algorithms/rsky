use crate::oauth::SharedOAuthProvider;
use http::header;
use rocket::data::{FromData, ToByteUnit};
use rocket::http::Status;
use rocket::serde::json::Json;
use rocket::{post, Data, Request, State};
use rsky_oauth::oauth_provider::errors::OAuthError;
use rsky_oauth::oauth_types::{
    OAuthClientCredentials, OAuthIntrospectionResponse, OAuthTokenIdentification,
};
use std::num::NonZeroU64;

#[post("/oauth/introspect", data = "<body>")]
pub async fn oauth_introspect(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    body: OAuthIntrospectRequestBody,
) -> Result<Json<OAuthIntrospectionResponse>, OAuthError> {
    unimplemented!();
    // let mut oauth_provider = shared_oauth_provider.oauth_provider.write().await;
    // let res = oauth_provider
    //     .introspect(
    //         body.oauth_client_credentials,
    //         body.oauth_token_identification,
    //     )
    //     .await?;
    // Ok(Json(res))
}
