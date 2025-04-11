use crate::account_manager::AccountManager;
use crate::oauth::{SharedOAuthProvider, SharedReplayStore};
use rocket::serde::json::Json;
use rocket::{get, State};
use rsky_oauth::oauth_provider::errors::OAuthError;
use rsky_oauth::oauth_types::OAuthAuthorizationServerMetadata;
use std::sync::Arc;
use tokio::sync::RwLock;

#[get("/.well-known/oauth-authorization-server")]
pub async fn oauth_well_known(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    shared_replay_store: &State<SharedReplayStore>,
    account_manager: AccountManager,
) -> Result<Json<OAuthAuthorizationServerMetadata>, OAuthError> {
    let creator = shared_oauth_provider.oauth_provider.read().await;
    let x = Arc::new(RwLock::new(account_manager));
    let oauth_provider = creator(
        x.clone(),
        Some(x.clone()),
        x.clone(),
        x.clone(),
        Some(x.clone()),
        Some(shared_replay_store.replay_store.clone()),
    );
    Ok(Json(oauth_provider.metadata.clone()))
}
