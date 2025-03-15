use super::SimpleStore;
use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::{Arc, Mutex};
use thiserror::Error;

#[derive(Error, Debug)]
#[error("memory store error")]
pub struct Error;

// TODO: LRU cache?
pub struct MemorySimpleStore<K, V> {
    store: Arc<Mutex<HashMap<K, V>>>,
}

impl<K, V> Default for MemorySimpleStore<K, V> {
    fn default() -> Self {
        Self {
            store: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl<K, V> SimpleStore<K, V> for MemorySimpleStore<K, V>
where
    K: Debug + Eq + Hash + Send + Sync + 'static,
    V: Debug + Clone + Send + Sync + 'static,
{
    type Error = Error;

    async fn get(&self, key: &K) -> Result<Option<V>, Self::Error> {
        Ok(self.store.lock().unwrap().get(key).cloned())
    }
    async fn set(&self, key: K, value: V) -> Result<(), Self::Error> {
        self.store.lock().unwrap().insert(key, value);
        Ok(())
    }
    async fn del(&self, key: &K) -> Result<(), Self::Error> {
        self.store.lock().unwrap().remove(key);
        Ok(())
    }
    async fn clear(&self) -> Result<(), Self::Error> {
        self.store.lock().unwrap().clear();
        Ok(())
    }
}
