use crate::account_manager::AccountManager;
use crate::oauth::{SharedOAuthProvider, SharedReplayStore};
use jsonwebtoken::jwk::JwkSet;
use rocket::serde::json::Json;
use rocket::{get, State};
use std::sync::Arc;
use tokio::sync::RwLock;

#[get("/oauth/jwks")]
pub async fn oauth_jwks(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    shared_replay_store: &State<SharedReplayStore>,
    account_manager: AccountManager,
) -> Json<JwkSet> {
    let creator = shared_oauth_provider.oauth_provider.read().await;
    let account_manager_lock = Arc::new(RwLock::new(account_manager));
    let oauth_provider = creator(
        account_manager_lock.clone(),
        Some(account_manager_lock.clone()),
        account_manager_lock.clone(),
        account_manager_lock.clone(),
        Some(account_manager_lock.clone()),
        Some(shared_replay_store.replay_store.clone()),
    );
    let result = oauth_provider.get_jwks().await;

    Json(JwkSet { keys: result })
}
