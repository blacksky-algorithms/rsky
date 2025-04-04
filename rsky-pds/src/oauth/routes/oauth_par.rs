use crate::account_manager::AccountManager;
use crate::oauth::routes::DpopJkt;
use crate::oauth::{SharedOAuthProvider, SharedReplayStore};
use http::header;
use rocket::data::{FromData, ToByteUnit};
use rocket::http::Status;
use rocket::serde::json::Json;
use rocket::{post, Data, Request, State};
use rsky_oauth::oauth_provider::errors::OAuthError;
use rsky_oauth::oauth_provider::OAuthParRequestBody;
use rsky_oauth::oauth_types::{
    OAuthAuthorizationRequestPar, OAuthClientCredentials, OAuthParResponse,
};
use std::num::NonZeroU64;
use std::sync::Arc;
use tokio::sync::RwLock;

#[post("/oauth/par", data = "<body>")]
pub async fn oauth_par(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    shared_replay_store: &State<SharedReplayStore>,
    account_manager: AccountManager,
    body: OAuthParRequestBody,
    dpop_jkt: DpopJkt,
) -> Result<Json<OAuthParResponse>, OAuthError> {
    let creator = shared_oauth_provider.oauth_provider.read().await;
    let x = Arc::new(RwLock::new(account_manager));
    let mut oauth_provider = creator(
        x.clone(),
        x.clone(),
        x.clone(),
        x.clone(),
        x.clone(),
        shared_replay_store.replay_store.clone(),
    );
    let dpop_jkt = oauth_provider
        .oauth_verifier
        .check_dpop_proof(
            dpop_jkt.0.unwrap().as_str(),
            "POST",
            body.url.as_str(),
            None,
        )
        .await?;
    let res = oauth_provider
        .pushed_authorization_request(
            body.oauth_client_credentials.clone(),
            body.oauth_authorization_request_par.clone(),
            Some(dpop_jkt),
        )
        .await?;
    Ok(Json(res))
}
