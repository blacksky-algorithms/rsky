use crate::oauth_provider::constants::{
    CLIENT_ASSERTION_MAX_AGE, CODE_CHALLENGE_REPLAY_TIMEFRAME, DPOP_NONCE_MAX_AGE, JAR_MAX_AGE,
};
use crate::oauth_provider::replay::replay_store::ReplayStore;
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
        self.replay_store.blocking_write().unique(
            format!("Auth@{client_id}").as_str(),
            jti.as_str(),
            as_time_frame(CLIENT_ASSERTION_MAX_AGE as f64),
        )
    }

    pub async fn unique_jar(&mut self, jti: String, client_id: &OAuthClientId) -> bool {
        self.replay_store.blocking_write().unique(
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
        self.replay_store.blocking_write().unique(
            namespace.as_str(),
            jti.as_str(),
            as_time_frame(DPOP_NONCE_MAX_AGE as f64),
        )
    }

    pub async fn unique_code_challenge(&mut self, challenge: String) -> bool {
        self.replay_store.blocking_write().unique(
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
