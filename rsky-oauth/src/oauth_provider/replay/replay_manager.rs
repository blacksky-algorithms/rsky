use crate::oauth_provider::client::client_id::ClientId;
use crate::oauth_provider::constants::{
    CLIENT_ASSERTION_MAX_AGE, CODE_CHALLENGE_REPLAY_TIMEFRAME, DPOP_NONCE_MAX_AGE, JAR_MAX_AGE,
};
use crate::oauth_provider::replay::replay_store_memory::ReplayStoreMemory;

pub struct ReplayManager {
    replay_store: ReplayStoreMemory, // get ipld blocks from db
}

impl ReplayManager {
    pub fn new(replay_store: ReplayStoreMemory) -> Self {
        ReplayManager { replay_store }
    }

    pub async fn unique_auth(&mut self, jti: String, client_id: &ClientId) -> bool {
        self.replay_store
            .unique(
                format!("Auth@{client_id}").as_str(),
                jti.as_str(),
                as_time_frame(CLIENT_ASSERTION_MAX_AGE as f64),
            )
            .await
    }

    pub async fn unique_jar(&mut self, jti: String, client_id: String) -> bool {
        self.replay_store
            .unique(
                format!("JAR@{client_id}").as_str(),
                jti.as_str(),
                as_time_frame(JAR_MAX_AGE as f64),
            )
            .await
    }

    pub async fn unique_dpop(&mut self, jti: String, client_id: Option<String>) -> bool {
        let namespace = match client_id {
            Some(res) => {
                format!("DPoP@{res}")
            }
            None => "DPoP".to_string(),
        };
        self.replay_store
            .unique(
                namespace.as_str(),
                jti.as_str(),
                as_time_frame(DPOP_NONCE_MAX_AGE as f64),
            )
            .await
    }

    pub async fn unique_code_challenge(&mut self, challenge: String) -> bool {
        self.replay_store
            .unique(
                "CodeChallenge",
                challenge.as_str(),
                as_time_frame(CODE_CHALLENGE_REPLAY_TIMEFRAME as f64),
            )
            .await
    }
}

const SECURITY_RATIO: f64 = 1.1; // 10% extra time for security

fn as_time_frame(time_frame: f64) -> f64 {
    (time_frame * SECURITY_RATIO).ceil()
}
