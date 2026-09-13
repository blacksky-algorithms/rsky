use redis::aio::{ConnectionManager, ConnectionManagerConfig};
use rsky_oauth::dpop::ReplayStore;
use rsky_oauth::OAuthError;
use std::time::Duration;
use tokio::sync::Mutex;

/// DPoP replay tracking in redis under the reference PDS's key scheme, so a
/// proof consumed by one process is rejected by every other one.
pub struct RedisReplayStore {
    connection: Mutex<ConnectionManager>,
}

impl RedisReplayStore {
    pub async fn connect(url: &str) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(url)?;
        // reconnects are bounded so a lost redis surfaces as an error on
        // the request instead of stalling it
        let config = ConnectionManagerConfig::new()
            .set_number_of_retries(3)
            .set_max_delay(1_000)
            .set_connection_timeout(Duration::from_secs(5))
            .set_response_timeout(Duration::from_secs(5));
        let connection = ConnectionManager::new_with_config(client, config).await?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn key(namespace: &str, nonce: &str) -> String {
        format!("nonces:{namespace}:{nonce}")
    }
}

#[async_trait::async_trait]
impl ReplayStore for RedisReplayStore {
    async fn unique(
        &self,
        namespace: &str,
        nonce: &str,
        time_frame_ms: u64,
    ) -> Result<bool, OAuthError> {
        let mut connection = self.connection.lock().await;
        let previous: Option<String> = redis::cmd("SET")
            .arg(Self::key(namespace, nonce))
            .arg("1")
            .arg("PX")
            .arg(time_frame_ms)
            .arg("GET")
            .query_async(&mut *connection)
            .await
            .map_err(|error| OAuthError::ServerError(format!("replay store: {error}")))?;
        Ok(previous.is_none())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs against the redis named by `TEST_REDIS_URL`; the store is
    /// exercised for real, so the test is skipped without one.
    #[tokio::test]
    async fn redis_store_tracks_nonces_per_namespace() {
        let Ok(url) = std::env::var("TEST_REDIS_URL") else {
            return;
        };
        let store = RedisReplayStore::connect(&url).await.unwrap();
        let nonce = format!("jti-{}", rsky_common::get_random_str());
        assert!(store.unique("DPoP", &nonce, 60_000).await.unwrap());
        assert!(!store.unique("DPoP", &nonce, 60_000).await.unwrap());
        assert!(store.unique("DPoP@client", &nonce, 60_000).await.unwrap());
        assert!(store
            .unique("DPoP", &format!("{nonce}-short"), 1)
            .await
            .unwrap());
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert!(store
            .unique("DPoP", &format!("{nonce}-short"), 1)
            .await
            .unwrap());
        assert_eq!(RedisReplayStore::key("DPoP", "x"), "nonces:DPoP:x");
        let broken = RedisReplayStore::connect("redis://127.0.0.1:1").await;
        assert!(broken.is_err());
    }
}
