use crate::jwk::Keyset;
use crate::oauth_provider::replay::replay_store::ReplayStore;
use crate::oauth_provider::signer::signer::{Signer, SignerCreator};
use crate::oauth_types::OAuthIssuerIdentifier;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::SystemTime;

pub struct ReplayStoreMemory {
    last_cleanup: f64,
    nonces: BTreeMap<String, f64>,
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
    fn unique(&mut self, namespace: &str, nonce: &str, timeframe: f64) -> bool {
        self.cleanup();
        let key = format!("{namespace}:{nonce}");
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("timestamp in micros since UNIX epoch")
            .as_micros() as f64;
        self.nonces.insert(key, now + timeframe) == None
    }
}
