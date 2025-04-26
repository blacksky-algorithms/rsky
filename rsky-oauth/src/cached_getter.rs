use crate::simple_store::SimpleStore;
use crate::simple_store_memory::SimpleStoreMemory;
use std::collections::HashMap;
use std::fmt::Debug;
use std::future::Future;
use std::hash::Hash;
use std::pin::Pin;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Error, Debug)]
#[error("Cached Getter error")]
pub struct CachedGetterError;

#[derive(Clone)]
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

pub trait Getter<K, V>: Send + Sync {
    fn get<'a>(
        &'a self,
        key: K,
        options: Option<GetCachedOptions>,
        stored_value: Option<V>,
    ) -> Pin<Box<dyn Future<Output = V> + Send + Sync + 'a>>;
}

pub struct CachedGetterOptions<K, V> {
    pub is_stale: Option<Pin<Box<dyn Fn(K, V) -> bool + Send + Sync>>>,
    pub on_store_error: Option<Pin<Box<dyn Fn(K, V) -> bool + Send + Sync>>>,
    pub delete_on_error: Option<Pin<Box<dyn Fn(K, V) -> bool + Send + Sync>>>,
}

pub struct PendingItem<V> {
    pub value: V,
    pub is_fresh: bool,
}

pub struct CachedGetter<K, V>
where
    K: Debug + Eq + Hash + Send + Sync + Clone,
    V: Debug + Clone + Send + Sync,
{
    getter: Arc<RwLock<dyn Getter<K, V>>>,
    pending: Arc<RwLock<HashMap<K, PendingItem<V>>>>,
    store: Arc<RwLock<SimpleStoreMemory<K, V>>>,
    options: Option<CachedGetterOptions<K, V>>,
}

/**
 * Wrapper utility that uses a store to speed up the retrieval of values from an
 * (expensive) getter function.
 */
impl<K: Eq + Hash + Debug + Send + Sync + Clone, V: Clone + Sync + Debug + Send>
    CachedGetter<K, V>
{
    pub fn new(
        getter: Arc<RwLock<dyn Getter<K, V>>>,
        store: Arc<RwLock<SimpleStoreMemory<K, V>>>,
        options: Option<CachedGetterOptions<K, V>>,
    ) -> Self {
        Self {
            getter,
            pending: Arc::new(RwLock::new(HashMap::new())),
            store,
            options,
        }
    }

    pub async fn get(&self, key: &K, options: Option<GetCachedOptions>) -> V {
        let is_stale = match &self.options {
            None => None,
            Some(options) => match &options.is_stale {
                None => None,
                Some(is_stale) => Some(is_stale),
            },
        };

        let allow_stored = Box::pin(|value: V| -> bool {
            return match options.clone() {
                None => match is_stale {
                    None => true,
                    Some(is_stale) => is_stale(key.clone(), value),
                },
                Some(options) => {
                    if options.no_cache {
                        return false;
                    }
                    if options.allow_stale {
                        return true;
                    }
                    match is_stale {
                        None => true,
                        Some(is_stale) => is_stale(key.clone(), value),
                    }
                }
            };
        });

        let pending = self.pending.read().await;
        match pending.get(&key) {
            None => {}
            Some(pending) => {
                if pending.is_fresh {
                    return pending.value.clone();
                }
                if allow_stored.clone()(pending.value.clone()) {
                    return pending.value.clone();
                }
            }
        }

        let stored_value = self.get_stored(&key, options.clone()).await;
        match &stored_value {
            None => {}
            Some(stored_value) => {
                if allow_stored.clone()(stored_value.clone()) {
                    let x = PendingItem::<V> {
                        value: stored_value.clone(),
                        is_fresh: false,
                    };
                    return x.value;
                }
            }
        }

        let getter = self.getter.read().await;
        getter.get(key.clone(), options, stored_value).await
    }

    pub async fn get_stored(&self, key: &K, options: Option<GetCachedOptions>) -> Option<V> {
        let store = self.store.read().await;
        store.get(key).await.unwrap_or_else(|_| None)
    }

    pub async fn set_stored(&self, key: K, value: V) {
        let store = self.store.read().await;
        match store.set(key.clone(), value.clone()).await {
            Ok(_) => {}
            Err(_) => {
                if let Some(options) = &self.options {
                    if let Some(on_store_error) = &options.on_store_error {
                        on_store_error(key, value);
                    }
                }
            }
        }
    }

    pub async fn del_stored(&self, key: &K) {
        let store = self.store.read().await;
        store.del(key).await.unwrap()
    }
}
