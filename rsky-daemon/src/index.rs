//! The synced permissioned-record index the daemon maintains, and which the
//! appview reads. Abstracted behind a trait; an in-memory implementation backs
//! tests and the eventual Postgres index reuses the same interface.

use async_trait::async_trait;
use rsky_space::LtHash;
use std::collections::HashMap;
use std::sync::RwLock;

use crate::error::Result;

/// A stored record's minimal index entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedRecord {
    pub cid: String,
    pub rev: String,
    pub value: Option<Vec<u8>>,
}

/// Per-author sync state + records the daemon holds for a space.
#[async_trait]
pub trait SpaceIndex: Send + Sync {
    /// The last commit revision indexed for this author, if any.
    async fn last_rev(&self, did: &str) -> Result<Option<String>>;
    /// The persisted LtHash accumulator for this author (empty if unknown).
    async fn load_lthash(&self, did: &str) -> Result<LtHash>;
    /// Current CID for a path, used to remove the prior element on update/delete.
    async fn get_cid(&self, did: &str, collection: &str, rkey: &str) -> Result<Option<String>>;
    /// Insert or replace a record.
    async fn upsert(
        &self,
        did: &str,
        collection: &str,
        rkey: &str,
        cid: &str,
        rev: &str,
        value: Option<Vec<u8>>,
    ) -> Result<()>;
    /// Remove a record.
    async fn delete(&self, did: &str, collection: &str, rkey: &str) -> Result<()>;
    /// Persist the author's new head (rev + accumulator) after a synced batch.
    async fn save_head(&self, did: &str, rev: &str, lthash: &LtHash) -> Result<()>;
}

fn key(collection: &str, rkey: &str) -> String {
    format!("{collection}/{rkey}")
}

#[derive(Default)]
struct AuthorState {
    rev: Option<String>,
    state_bytes: Option<[u8; 2048]>,
    records: HashMap<String, IndexedRecord>,
}

/// In-memory [`SpaceIndex`] for tests and local runs.
#[derive(Default)]
pub struct InMemoryIndex {
    authors: RwLock<HashMap<String, AuthorState>>,
}

impl InMemoryIndex {
    pub fn new() -> Self {
        Self::default()
    }
    /// Test/inspection helper: fetch a stored record.
    pub fn record(&self, did: &str, collection: &str, rkey: &str) -> Option<IndexedRecord> {
        self.authors
            .read()
            .unwrap()
            .get(did)
            .and_then(|a| a.records.get(&key(collection, rkey)).cloned())
    }
    pub fn record_count(&self, did: &str) -> usize {
        self.authors
            .read()
            .unwrap()
            .get(did)
            .map(|a| a.records.len())
            .unwrap_or(0)
    }
}

#[async_trait]
impl SpaceIndex for InMemoryIndex {
    async fn last_rev(&self, did: &str) -> Result<Option<String>> {
        Ok(self
            .authors
            .read()
            .unwrap()
            .get(did)
            .and_then(|a| a.rev.clone()))
    }

    async fn load_lthash(&self, did: &str) -> Result<LtHash> {
        Ok(self
            .authors
            .read()
            .unwrap()
            .get(did)
            .and_then(|a| a.state_bytes)
            .map(|b| LtHash::from_state_bytes(&b))
            .unwrap_or_default())
    }

    async fn get_cid(&self, did: &str, collection: &str, rkey: &str) -> Result<Option<String>> {
        Ok(self
            .authors
            .read()
            .unwrap()
            .get(did)
            .and_then(|a| a.records.get(&key(collection, rkey)))
            .map(|r| r.cid.clone()))
    }

    async fn upsert(
        &self,
        did: &str,
        collection: &str,
        rkey: &str,
        cid: &str,
        rev: &str,
        value: Option<Vec<u8>>,
    ) -> Result<()> {
        let mut authors = self.authors.write().unwrap();
        let a = authors.entry(did.to_string()).or_default();
        a.records.insert(
            key(collection, rkey),
            IndexedRecord {
                cid: cid.to_string(),
                rev: rev.to_string(),
                value,
            },
        );
        Ok(())
    }

    async fn delete(&self, did: &str, collection: &str, rkey: &str) -> Result<()> {
        if let Some(a) = self.authors.write().unwrap().get_mut(did) {
            a.records.remove(&key(collection, rkey));
        }
        Ok(())
    }

    async fn save_head(&self, did: &str, rev: &str, lthash: &LtHash) -> Result<()> {
        let mut authors = self.authors.write().unwrap();
        let a = authors.entry(did.to_string()).or_default();
        a.rev = Some(rev.to_string());
        a.state_bytes = Some(lthash.state_bytes());
        Ok(())
    }
}
