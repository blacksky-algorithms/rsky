use crate::account_manager::AccountManager;
use crate::oauth::{SharedOAuthProvider, SharedReplayStore};
use jsonwebtoken::jwk::Jwk;
use rocket::serde::json::Json;
use rocket::{get, State};
use std::sync::Arc;
use tokio::sync::RwLock;

#[get("/oauth/jwks")]
pub async fn oauth_jwks(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    shared_replay_store: &State<SharedReplayStore>,
    account_manager: AccountManager,
) -> Json<Vec<Jwk>> {
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
    Json(oauth_provider.get_jwks().await)
}
