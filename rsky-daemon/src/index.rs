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

/// One record change from a synced batch, as journaled for projection.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum IndexMutation {
    Upsert {
        collection: String,
        rkey: String,
        cid: String,
        rev: String,
        value: Option<Vec<u8>>,
    },
    Delete {
        collection: String,
        rkey: String,
    },
}

impl IndexMutation {
    pub fn collection(&self) -> &str {
        match self {
            Self::Upsert { collection, .. } | Self::Delete { collection, .. } => collection,
        }
    }
    pub fn rkey(&self) -> &str {
        match self {
            Self::Upsert { rkey, .. } | Self::Delete { rkey, .. } => rkey,
        }
    }
}

/// A journaled batch a projector has not yet delivered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournaledBatch {
    pub author: String,
    pub rev: String,
    pub mutations: Vec<IndexMutation>,
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
    /// Record a synced batch for later projection, keyed by `(did, rev)`.
    /// Journaling precedes the head write, so a crash between the two replays
    /// the batch instead of losing it; the key makes that replay a no-op.
    async fn journal_batch(
        &self,
        _did: &str,
        _rev: &str,
        _mutations: &[IndexMutation],
    ) -> Result<()> {
        Ok(())
    }
    /// Batches past this projector's independent `(author, rev)` cursor.
    async fn pending_batches(&self, _projector: &str) -> Result<Vec<JournaledBatch>> {
        Ok(Vec::new())
    }
    async fn advance_projector_cursor(
        &self,
        _projector: &str,
        _did: &str,
        _rev: &str,
    ) -> Result<()> {
        Ok(())
    }
    /// Returns the durable failure count for this batch, marking it
    /// dead-lettered once the count reaches `dead_letter_after`.
    async fn record_projection_failure(
        &self,
        _projector: &str,
        _did: &str,
        _rev: &str,
        _error: &str,
        _dead_letter_after: u32,
    ) -> Result<u32> {
        Ok(0)
    }
    /// Drop journal rows every one of `projectors` has advanced past, along
    /// with their retryable failure rows. Dead-lettered batches are retained
    /// until explicitly cleared.
    async fn prune_journal(&self, _projectors: &[&str]) -> Result<usize> {
        Ok(0)
    }
    /// Enumerate an author's indexed records as `(collection, rkey, cid)`,
    /// used to diff against a recovered full-state CAR.
    async fn list_paths(&self, did: &str) -> Result<Vec<(String, String, String)>>;
    /// Drop every record and sync head this index holds. A syncer MUST delete
    /// all data for a deleted space (proposal §Space deletion).
    async fn purge_space(&self) -> Result<()>;
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

#[derive(Default)]
struct JournalState {
    batches: Vec<JournaledBatch>,
    cursors: HashMap<(String, String), String>,
    failures: HashMap<(String, String, String), (u32, bool)>,
}

/// In-memory [`SpaceIndex`] for tests and local runs.
#[derive(Default)]
pub struct InMemoryIndex {
    authors: RwLock<HashMap<String, AuthorState>>,
    journal: RwLock<JournalState>,
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

    async fn journal_batch(&self, did: &str, rev: &str, mutations: &[IndexMutation]) -> Result<()> {
        let mut journal = self.journal.write().unwrap();
        if journal
            .batches
            .iter()
            .any(|b| b.author == did && b.rev == rev)
        {
            return Ok(());
        }
        journal.batches.push(JournaledBatch {
            author: did.to_string(),
            rev: rev.to_string(),
            mutations: mutations.to_vec(),
        });
        Ok(())
    }

    async fn pending_batches(&self, projector: &str) -> Result<Vec<JournaledBatch>> {
        let journal = self.journal.read().unwrap();
        let mut pending: Vec<JournaledBatch> = journal
            .batches
            .iter()
            .filter(|b| {
                journal
                    .cursors
                    .get(&(projector.to_string(), b.author.clone()))
                    .is_none_or(|cursor| b.rev > *cursor)
                    && !journal
                        .failures
                        .get(&(projector.to_string(), b.author.clone(), b.rev.clone()))
                        .is_some_and(|(_, dead)| *dead)
            })
            .cloned()
            .collect();
        pending.sort_by(|a, b| (&a.author, &a.rev).cmp(&(&b.author, &b.rev)));
        Ok(pending)
    }

    async fn advance_projector_cursor(&self, projector: &str, did: &str, rev: &str) -> Result<()> {
        self.journal
            .write()
            .unwrap()
            .cursors
            .insert((projector.to_string(), did.to_string()), rev.to_string());
        Ok(())
    }

    async fn record_projection_failure(
        &self,
        projector: &str,
        did: &str,
        rev: &str,
        _error: &str,
        dead_letter_after: u32,
    ) -> Result<u32> {
        let mut journal = self.journal.write().unwrap();
        let entry = journal
            .failures
            .entry((projector.to_string(), did.to_string(), rev.to_string()))
            .or_insert((0, false));
        entry.0 += 1;
        entry.1 = entry.0 >= dead_letter_after;
        Ok(entry.0)
    }

    async fn prune_journal(&self, projectors: &[&str]) -> Result<usize> {
        if projectors.is_empty() {
            return Ok(0);
        }
        let mut journal = self.journal.write().unwrap();
        let JournalState {
            batches,
            cursors,
            failures,
        } = &mut *journal;
        let before = batches.len();
        batches.retain(|b| {
            let all_passed = projectors.iter().all(|projector| {
                cursors
                    .get(&(projector.to_string(), b.author.clone()))
                    .is_some_and(|cursor| *cursor >= b.rev)
            });
            let dead_lettered = projectors.iter().any(|projector| {
                failures
                    .get(&(projector.to_string(), b.author.clone(), b.rev.clone()))
                    .is_some_and(|(_, dead)| *dead)
            });
            !all_passed || dead_lettered
        });
        let pruned = before - batches.len();
        failures.retain(|(_, author, rev), (_, dead)| {
            *dead || batches.iter().any(|b| b.author == *author && b.rev == *rev)
        });
        Ok(pruned)
    }

    async fn list_paths(&self, did: &str) -> Result<Vec<(String, String, String)>> {
        Ok(self
            .authors
            .read()
            .unwrap()
            .get(did)
            .map(|a| {
                a.records
                    .iter()
                    .map(|(path, r)| {
                        let (collection, rkey) = path.split_once('/').expect("keyed by path");
                        (collection.to_string(), rkey.to_string(), r.cid.clone())
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn purge_space(&self) -> Result<()> {
        self.authors.write().unwrap().clear();
        *self.journal.write().unwrap() = JournalState::default();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn list_paths_and_purge_space() {
        let index = InMemoryIndex::new();
        assert!(index.list_paths("did:plc:a").await.unwrap().is_empty());
        index
            .upsert("did:plc:a", "c.o.l", "3ka", "bafyA", "3rev", None)
            .await
            .unwrap();
        index
            .save_head("did:plc:a", "3rev", &LtHash::new())
            .await
            .unwrap();
        assert_eq!(
            index.list_paths("did:plc:a").await.unwrap(),
            vec![("c.o.l".to_string(), "3ka".to_string(), "bafyA".to_string())]
        );

        index.purge_space().await.unwrap();
        assert_eq!(index.record_count("did:plc:a"), 0);
        assert_eq!(index.last_rev("did:plc:a").await.unwrap(), None);
    }
}
