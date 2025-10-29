use crate::indexing::RecordPlugin;
use crate::IndexerError;
use async_trait::async_trait;
use deadpool_postgres::Pool;
use serde_json::Value as JsonValue;

pub struct FollowPlugin;

#[async_trait]
impl RecordPlugin for FollowPlugin {
    fn collection(&self) -> &str {
        "app.bsky.graph.follow"
    }

    async fn insert(&self, pool: &Pool, uri: &str, cid: &str, _record: &JsonValue, _timestamp: &str) -> Result<(), IndexerError> {
        let client = pool.get().await?;
        client.execute("INSERT INTO follow (uri, cid) VALUES ($1, $2) ON CONFLICT DO NOTHING", &[&uri, &cid]).await.map_err(|e| IndexerError::Database(e.into()))?;
        Ok(())
    }

    async fn update(&self, _pool: &Pool, _uri: &str, _cid: &str, _record: &JsonValue, _timestamp: &str) -> Result<(), IndexerError> {
        Ok(())
    }

    async fn delete(&self, pool: &Pool, uri: &str) -> Result<(), IndexerError> {
        let client = pool.get().await?;
        client.execute("DELETE FROM follow WHERE uri = $1", &[&uri]).await.map_err(|e| IndexerError::Database(e.into()))?;
        Ok(())
    }
}
