use crate::simple_store::SimpleStore;
use crate::simple_store_memory::{MemoryStoreError, SimpleStoreMemory};
use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::{Arc, Mutex, RwLock};
use thiserror::Error;

#[derive(Error, Debug)]
#[error("Cached Getter error")]
pub struct CachedGetterError;

pub struct GetCachedOptions {
    /**
     * Do not use the cache to get the value. Always get a new value from the
     * getter function.
     *
     * @default false
     */
    pub no_cache: bool,

    /**
     * When getting a value from the cache, allow the value to be returned even if
     * it is stale.
     *
     * Has no effect if the `isStale` option was not provided to the CachedGetter.
     *
     * @default true // If the CachedGetter has an isStale option
     * @default false // If no isStale option was provided to the CachedGetter
     */
    pub allow_stale: bool,
}

pub type Getter<K, V> = Box<dyn Fn(K, Option<GetCachedOptions>, Option<V>) -> V + Send + Sync>;

pub struct CachedGetterOptions<K, V> {
    pub is_stale: Option<Box<dyn Fn(K, V) -> bool + Send + Sync>>,
    pub on_store_error: Option<Box<dyn Fn(K, V)>>,
    pub delete_on_error: Option<Box<dyn Fn(K, V) -> bool + Send + Sync>>,
}

pub struct PendingItem<V> {
    pub value: V,
    pub is_fresh: bool,
}

pub struct CachedGetter<K, V> {
    pending: Arc<RwLock<HashMap<K, PendingItem<V>>>>,
    store: SimpleStoreMemory<K, V>,
    getter: Getter<K, V>,
    options: Option<CachedGetterOptions<K, V>>,
}

/**
 * Wrapper utility that uses a store to speed up the retrieval of values from an
 * (expensive) getter function.
 */
impl<K, V> CachedGetter<K, V> {
    pub fn new(
        getter: Getter<K, V>,
        store: SimpleStoreMemory<K, V>,
        options: Option<CachedGetterOptions<K, V>>,
    ) -> Self {
        Self {
            pending: Arc::new(RwLock::new(HashMap::new())),
            store,
            getter,
            options,
        }
    }

    pub async fn get(&self, key: &K, options: Option<GetCachedOptions>) -> Option<V> {
        let is_stale = match &self.options {
            None => None,
            Some(options) => options.is_stale.clone(),
        };
    }

    pub async fn get_stored(&self, key: &K, options: Option<GetCachedOptions>) -> Option<V> {
        self.store.get(key).await.unwrap_or_else(|_| None)
    }

    pub async fn set_stored(&self, key: &K, value: &V) {
        match self.store.set(key, value).await {
            Ok(_) => {}
            Err(_) => {
                if let Some(options) = &self.options {
                    if let Some(on_store_error) = &options.on_store_error {
                        on_store_error(key, value)
                    }
                }
            }
        }
    }

    pub async fn del_stored(&self, key: &K) {
        self.store.del(key).await
    }
}
