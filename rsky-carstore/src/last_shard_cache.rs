use crate::meta::CarShard;
use rsky_common::models::Uid;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Debug, Error)]
pub enum LastShardError {
    #[error("LastShardError: {0}")]
    Error(String),
}

pub trait LastShardSource: Send + Sync {
    fn get_last_shard<'a>(
        &'a self,
        uid: &'a Uid,
    ) -> Pin<Box<dyn Future<Output = Result<Arc<CarShard>, LastShardError>> + Send + Sync + 'a>>;
}

#[derive(Clone)]
pub struct LastShardCache {
    pub source: Arc<dyn LastShardSource>,
    pub last_shard_cache: Arc<Mutex<HashMap<Uid, Arc<CarShard>>>>,
}

impl LastShardCache {
    pub async fn init(&self) {
        let mut cache = self.last_shard_cache.lock().await;
        *cache = HashMap::new();
    }

    pub async fn check(&self, user: &Uid) -> Option<Arc<CarShard>> {
        let cache = self.last_shard_cache.lock().await;
        match cache.get(user) {
            None => None,
            Some(last_shard) => Some(last_shard.clone()),
        }
    }

    pub async fn remove(&self, user: &Uid) {
        let mut cache = self.last_shard_cache.lock().await;
        let _ = cache.remove(user);
    }

    pub async fn put(&self, last_shard: Arc<CarShard>) {
        let mut cache = self.last_shard_cache.lock().await;
        let _ = cache.insert(last_shard.usr.clone(), last_shard);
    }

    pub async fn get(&self, user: &Uid) -> Result<Arc<CarShard>, LastShardError> {
        match self.check(user).await {
            Some(last_shard) => Ok(last_shard),
            None => {
                let last_shard = self.source.get_last_shard(user).await?;
                self.put(last_shard.clone()).await;
                Ok(last_shard)
            }
        }
    }
}
