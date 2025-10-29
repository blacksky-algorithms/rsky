use crate::indexing::RecordPlugin;
use crate::IndexerError;
use async_trait::async_trait;
use deadpool_postgres::Pool;
use serde_json::Value as JsonValue;

pub struct ProfilePlugin;

#[async_trait]
impl RecordPlugin for ProfilePlugin {
    fn collection(&self) -> &str {
        "app.bsky.actor.profile"
    }

    async fn insert(&self, pool: &Pool, uri: &str, _cid: &str, _record: &JsonValue, _timestamp: &str) -> Result<(), IndexerError> {
        let client = pool.get().await?;
        client.execute("INSERT INTO profile (uri) VALUES ($1) ON CONFLICT DO NOTHING", &[&uri]).await.map_err(|e| IndexerError::Database(e.into()))?;
        Ok(())
    }

    async fn update(&self, _pool: &Pool, _uri: &str, _cid: &str, _record: &JsonValue, _timestamp: &str) -> Result<(), IndexerError> {
        Ok(())
    }

    async fn delete(&self, pool: &Pool, uri: &str) -> Result<(), IndexerError> {
        let client = pool.get().await?;
        client.execute("DELETE FROM profile WHERE uri = $1", &[&uri]).await.map_err(|e| IndexerError::Database(e.into()))?;
        Ok(())
    }
}
