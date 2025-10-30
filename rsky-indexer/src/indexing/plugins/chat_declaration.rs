use crate::indexing::parse_timestamp;
use crate::indexing::RecordPlugin;
use crate::IndexerError;
use async_trait::async_trait;
use deadpool_postgres::Pool;
use serde_json::Value as JsonValue;

pub struct ChatDeclarationPlugin;

impl ChatDeclarationPlugin {
    /// Extract rkey from AT URI (format: at://did:plc:xyz/collection/rkey)
    fn extract_rkey(uri: &str) -> Option<String> {
        uri.rsplit('/').next().map(|s| s.to_string())
    }
}

#[async_trait]
impl RecordPlugin for ChatDeclarationPlugin {
    fn collection(&self) -> &str {
        "chat.bsky.actor.declaration"
    }

    async fn insert(
        &self,
        _pool: &Pool,
        uri: &str,
        _cid: &str,
        _record: &JsonValue,
        _timestamp: &str,
    ) -> Result<(), IndexerError> {
        // Placeholder plugin: only validates rkey === 'self'
        // No dedicated table - only generic record table is used
        let rkey = Self::extract_rkey(uri);
        if rkey.as_deref() != Some("self") {
            return Err(IndexerError::Serialization(format!(
                "ChatDeclaration record must have rkey 'self', got: {:?}",
                rkey
            )));
        }
        Ok(())
    }

    async fn update(
        &self,
        _pool: &Pool,
        _uri: &str,
        _cid: &str,
        _record: &JsonValue,
        _timestamp: &str,
    ) -> Result<(), IndexerError> {
        // No-op for placeholder plugins
        Ok(())
    }

    async fn delete(&self, _pool: &Pool, uri: &str) -> Result<(), IndexerError> {
        // Placeholder plugin: only validates rkey === 'self'
        let rkey = Self::extract_rkey(uri);
        if rkey.as_deref() != Some("self") {
            return Err(IndexerError::Serialization(format!(
                "ChatDeclaration record must have rkey 'self', got: {:?}",
                rkey
            )));
        }
        Ok(())
    }
}
