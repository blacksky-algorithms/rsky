use crate::simple_store::SimpleStore;
use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Error, Debug)]
#[error("memory store error")]
pub struct MemoryStoreError;

// TODO: LRU cache?
pub struct SimpleStoreMemory<K, V> {
    store: Arc<RwLock<HashMap<K, V>>>,
}

impl<K, V> Default for SimpleStoreMemory<K, V> {
    fn default() -> Self {
        Self {
            store: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl<K, V> SimpleStore<K, V> for SimpleStoreMemory<K, V>
where
    K: Debug + Eq + Hash + Send + Sync,
    V: Debug + Clone + Send + Sync,
{
    type Error = MemoryStoreError;

    async fn get(&self, key: &K) -> Result<Option<V>, Self::Error> {
        Ok(self.store.blocking_read().get(key).cloned())
    }
    async fn set(&self, key: K, value: V) -> Result<(), Self::Error> {
        self.store.blocking_write().insert(key, value);
        Ok(())
    }
    async fn del(&self, key: &K) -> Result<(), Self::Error> {
        self.store.blocking_write().remove(key);
        Ok(())
    }
    async fn clear(&self) -> Result<(), Self::Error> {
        self.store.blocking_write().clear();
        Ok(())
    }
}
