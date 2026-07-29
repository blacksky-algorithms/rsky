//! The member list backing the `member-list` policy.
//!
//! Only the `member-list` policy reads membership directly; under
//! `managing-app` the decision is routed to the managing app
//! (see [`crate::managing_app`]) and this store is not consulted.

use async_trait::async_trait;
use std::collections::HashSet;
use std::sync::RwLock;

use crate::error::Result;

/// Decides whether a user DID may read the space.
#[async_trait]
pub trait MembershipStore: Send + Sync {
    async fn is_member(&self, did: &str) -> Result<bool>;
}

/// In-memory membership set (tests, and a stand-in until the Postgres-backed
/// `blacksky-beta` reader is wired in).
#[derive(Default)]
pub struct InMemoryMembership {
    members: RwLock<HashSet<String>>,
}

impl InMemoryMembership {
    pub fn new<I: IntoIterator<Item = String>>(dids: I) -> Self {
        Self {
            members: RwLock::new(dids.into_iter().collect()),
        }
    }
    pub fn add(&self, did: &str) {
        self.members.write().unwrap().insert(did.to_string());
    }
    pub fn remove(&self, did: &str) {
        self.members.write().unwrap().remove(did);
    }
}

#[async_trait]
impl MembershipStore for InMemoryMembership {
    async fn is_member(&self, did: &str) -> Result<bool> {
        Ok(self.members.read().unwrap().contains(did))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn add_remove_membership() {
        let members = InMemoryMembership::default();
        assert!(!members.is_member("did:plc:user").await.unwrap());
        members.add("did:plc:user");
        assert!(members.is_member("did:plc:user").await.unwrap());
        members.remove("did:plc:user");
        assert!(!members.is_member("did:plc:user").await.unwrap());
    }
}
