use crate::oauth_provider::constants::{
    CLIENT_ASSERTION_MAX_AGE, CODE_CHALLENGE_REPLAY_TIMEFRAME, DPOP_NONCE_MAX_AGE, JAR_MAX_AGE,
};
use crate::oauth_provider::replay::replay_store::ReplayStore;
use crate::oauth_provider::token::token_id::TokenId;
use crate::oauth_types::OAuthClientId;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct ReplayManager {
    replay_store: Arc<RwLock<dyn ReplayStore>>, // get ipld blocks from db
}

pub type ReplayManagerCreator =
    Box<dyn Fn(Arc<RwLock<dyn ReplayStore>>) -> ReplayManager + Send + Sync>;

impl ReplayManager {
    pub fn creator() -> ReplayManagerCreator {
        Box::new(
            move |store: Arc<RwLock<dyn ReplayStore>>| -> ReplayManager {
                ReplayManager::new(store)
            },
        )
    }

    pub fn new(replay_store: Arc<RwLock<dyn ReplayStore>>) -> Self {
        ReplayManager { replay_store }
    }

    pub async fn unique_auth(&mut self, jti: String, client_id: &OAuthClientId) -> bool {
        let mut replay_store = self.replay_store.write().await;
        replay_store.unique(
            format!("Auth@{client_id}").as_str(),
            jti.as_str(),
            as_time_frame(CLIENT_ASSERTION_MAX_AGE as f64),
        )
    }

    pub async fn unique_jar(&mut self, jti: String, client_id: &OAuthClientId) -> bool {
        let mut replay_store = self.replay_store.write().await;
        replay_store.unique(
            format!("JAR@{client_id}").as_str(),
            jti.as_str(),
            as_time_frame(JAR_MAX_AGE as f64),
        )
    }

    pub async fn unique_dpop(&mut self, jti: String, client_id: Option<String>) -> bool {
        let namespace = match client_id {
            Some(res) => {
                format!("DPoP@{res}")
            }
            None => "DPoP".to_string(),
        };
        let mut replay_store = self.replay_store.write().await;
        replay_store.unique(
            namespace.as_str(),
            jti.as_str(),
            as_time_frame(DPOP_NONCE_MAX_AGE as f64),
        )
    }

    pub async fn unique_code_challenge(&mut self, challenge: String) -> bool {
        let mut replay_store = self.replay_store.write().await;
        replay_store.unique(
            "CodeChallenge",
            challenge.as_str(),
            as_time_frame(CODE_CHALLENGE_REPLAY_TIMEFRAME as f64),
        )
    }
}

const SECURITY_RATIO: f64 = 1.1; // 10% extra time for security

fn as_time_frame(time_frame: f64) -> f64 {
    (time_frame * SECURITY_RATIO).ceil()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oauth_provider::replay::replay_store_memory::ReplayStoreMemory;
    use crate::oauth_types::OAuthClientId;

    fn create_replay_manager() -> ReplayManager {
        let replay_store = Arc::new(RwLock::new(ReplayStoreMemory::new()));
        ReplayManager::new(replay_store)
    }

    #[tokio::test]
    async fn test_unique_auth() {
        let mut replay_manager = create_replay_manager();
        let jti = String::from("h6cir8v1iw:1jmosfp4komez");
        let client_id = OAuthClientId::new("client").unwrap();
        let result = replay_manager.unique_auth(jti, &client_id).await;
        assert_eq!(result, true);
        let jti = String::from("h6cir8v1iw:1jmosfp4komez");
        let client_id = OAuthClientId::new("client").unwrap();
        let result = replay_manager.unique_auth(jti, &client_id).await;
        assert_eq!(result, false);
    }

    #[tokio::test]
    async fn test_unique_jar() {
        let mut replay_manager = create_replay_manager();
        let jti = String::from("h6cir8v1iw:1jmosfp4komez");
        let client_id = OAuthClientId::new("client").unwrap();
        let result = replay_manager.unique_jar(jti, &client_id).await;
        assert_eq!(result, true);
        let jti = String::from("h6cir8v1iw:1jmosfp4komez");
        let client_id = OAuthClientId::new("client").unwrap();
        let result = replay_manager.unique_jar(jti, &client_id).await;
        assert_eq!(result, false);
    }

    #[tokio::test]
    async fn test_unique_dpop() {
        let mut replay_manager = create_replay_manager();
        let token_id = TokenId::generate();
        let client_id = Some("client".to_string());
        let result = replay_manager
            .unique_dpop(token_id.clone().val(), client_id)
            .await;
        assert_eq!(result, true);
        let client_id = Some("client".to_string());
        let result = replay_manager.unique_dpop(token_id.val(), client_id).await;
        assert_eq!(result, false);
    }

    #[tokio::test]
    async fn test_unique_code_challenge() {
        let mut replay_manager = create_replay_manager();
        let challenge = String::from("challenge");
        let result = replay_manager.unique_code_challenge(challenge).await;
        assert_eq!(result, true);
        let challenge = String::from("challenge");
        let result = replay_manager.unique_code_challenge(challenge).await;
        assert_eq!(result, false);
    }
}
