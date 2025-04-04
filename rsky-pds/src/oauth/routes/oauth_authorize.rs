use crate::account_manager::AccountManager;
use crate::oauth::{SharedOAuthProvider, SharedReplayStore};
use rocket::request::FromRequest;
use rocket::{get, Request, State};
use rsky_oauth::oauth_provider::device::device_manager::DeviceManager;
use rsky_oauth::oauth_provider::lib::http::request::validate_fetch_site;
use rsky_oauth::oauth_provider::OAuthAuthorizeRequestBody;
use std::sync::Arc;
use tokio::sync::RwLock;

#[get("/oauth/authorize")]
pub async fn oauth_authorize(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    shared_replay_store: &State<SharedReplayStore>,
    body: OAuthAuthorizeRequestBody,
    account_manager: AccountManager,
    device_manager: DeviceManager,
) {
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

    let data = oauth_provider.authorize().await;
}
