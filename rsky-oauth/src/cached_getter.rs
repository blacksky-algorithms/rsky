use crate::simple_store::SimpleStore;
use crate::simple_store_memory::SimpleStoreMemory;
use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
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

pub struct CachedGetterOptions<K, V> {
    pub is_stale: Option<Box<dyn Fn(K, V) -> bool + Send + Sync>>,
    pub on_store_error: Option<Box<dyn Fn(K, V) -> bool + Send + Sync>>,
    pub delete_on_error: Option<Box<dyn Fn(K, V) -> bool + Send + Sync>>,
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
        store: Arc<RwLock<SimpleStoreMemory<K, V>>>,
        options: Option<CachedGetterOptions<K, V>>,
    ) -> Self {
        Self {
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
                Some(is_stale) => Some(is_stale.clone()),
            },
        };

        let allow_stored = Box::new(|value: V| -> bool {
            unimplemented!()
            // return match options {
            //     None => match is_stale {
            //         None => true,
            //         Some(is_stale) => is_stale(key.clone(), value),
            //     },
            //     Some(options) => {
            //         if options.no_cache {
            //             return false;
            //         }
            //         if options.allow_stale {
            //             return true;
            //         }
            //         match is_stale {
            //             None => true,
            //             Some(is_stale) => is_stale(key.clone(), value),
            //         }
            //     }
            // };
        });

        match self.pending.blocking_read().get(&key) {
            None => {}
            Some(pending) => {
                if pending.is_fresh {
                    return pending.value.clone();
                }
                if allow_stored(pending.value.clone()) {
                    return pending.value.clone();
                }
            }
        }

        let stored_value = self.get_stored(&key, options.clone()).await;
        match &stored_value {
            None => {}
            Some(stored_value) => {
                if allow_stored(stored_value.clone()) {
                    let x = PendingItem::<V> {
                        value: stored_value.clone(),
                        is_fresh: false,
                    };
                    return x.value;
                }
            }
        }

        // let res = (self.getter)(key.clone(), options, stored_value.as_ref());
        unimplemented!()
    }

    pub async fn get_stored(&self, key: &K, options: Option<GetCachedOptions>) -> Option<V> {
        self.store
            .blocking_read()
            .get(key)
            .await
            .unwrap_or_else(|_| None)
    }

    pub async fn set_stored(&self, key: K, value: V) {
        match self
            .store
            .blocking_write()
            .set(key.clone(), value.clone())
            .await
        {
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
        self.store.blocking_write().del(key).await.unwrap()
    }
}
