use crate::oauth_provider::replay::replay_store::ReplayStore;
use std::collections::BTreeMap;
use std::time::SystemTime;

pub struct ReplayStoreMemory {
    last_cleanup: f64,
    nonces: BTreeMap<String, f64>,
}

impl Default for ReplayStoreMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl ReplayStoreMemory {
    pub fn new() -> Self {
        ReplayStoreMemory {
            last_cleanup: 0f64,
            nonces: Default::default(),
        }
    }

    pub fn cleanup(&mut self) {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("timestamp in micros since UNIX epoch")
            .as_micros() as f64;

        if self.last_cleanup < now - 60_000f64 {
            for (key, expires) in self.nonces.clone() {
                if expires < now {
                    self.nonces.remove(&key);
                }
            }
            self.last_cleanup = now;
        }
    }
}

impl ReplayStore for ReplayStoreMemory {
    /**
     * Returns true if the nonce is unique within the given time frame.
     */
    fn unique(&mut self, namespace: &str, nonce: &str, timeframe: f64) -> bool {
        self.cleanup();
        let key = format!("{namespace}:{nonce}");
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("timestamp in micros since UNIX epoch")
            .as_micros() as f64;
        self.nonces.insert(key, now + timeframe).is_none()
    }
}

#[cfg(test)]
mod tests {
    use crate::oauth_provider::replay::replay_store::ReplayStore;
    use crate::oauth_provider::replay::replay_store_memory::ReplayStoreMemory;

    fn create_replay_store() -> ReplayStoreMemory {
        ReplayStoreMemory::new()
    }

    #[test]
    fn test_unique_auth() {
        let mut replay_store = create_replay_store();
        let namespace = "namespace";
        let nonce = "nonce";
        let timeframe = 0f64;
        let result = replay_store.unique(namespace, nonce, timeframe);
        assert_eq!(result, true);
        let result = replay_store.unique(namespace, nonce, timeframe);
        assert_eq!(result, false);
    }
}
