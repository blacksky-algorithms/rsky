use rsky_oauth::jwk::Keyset;
use rsky_oauth::oauth_provider::oauth_provider::OAuthProviderCreator;
use rsky_oauth::oauth_provider::replay::replay_store::ReplayStore;
use std::sync::Arc;
use tokio::sync::RwLock;

pub mod detailed_account_store;
pub mod models;
pub mod provider;
pub mod routes;

pub struct SharedOAuthProvider {
    pub oauth_provider: Arc<RwLock<OAuthProviderCreator>>,
    pub keyset: Arc<RwLock<Keyset>>,
}

impl SharedOAuthProvider {
    pub fn new(
        oauth_provider: Arc<RwLock<OAuthProviderCreator>>,
        keyset: Arc<RwLock<Keyset>>,
    ) -> Self {
        Self {
            oauth_provider,
            keyset,
        }
    }
}

pub struct SharedReplayStore {
    pub replay_store: Arc<RwLock<dyn ReplayStore>>,
}
