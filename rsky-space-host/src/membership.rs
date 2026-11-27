//! Member-list storage (spec §memberspace).
//!
//! A memberspace authorizes a user when their member-list entry has the
//! corresponding `read` or `write` boolean set. Under `managedspace` the
//! decision moves to the managing app (see [`crate::managing_app`]) and this
//! store is not consulted.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::RwLock;

use crate::error::Result;

/// A member's read and write access to a space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MemberAccess {
    pub read: bool,
    pub write: bool,
}

impl MemberAccess {
    pub const NONE: Self = MemberAccess {
        read: false,
        write: false,
    };
    pub const READ_WRITE: Self = MemberAccess {
        read: true,
        write: true,
    };
}

/// The member list's answer for one DID: their `read`/`write` booleans, both
/// false for a non-member.
#[async_trait]
pub trait MembershipStore: Send + Sync {
    async fn member_access(&self, did: &str) -> Result<MemberAccess>;
}

/// In-memory membership set (tests, and a stand-in until the Postgres-backed
/// `blacksky-beta` reader is wired in).
#[derive(Default)]
pub struct InMemoryMembership {
    members: RwLock<HashMap<String, MemberAccess>>,
}

impl InMemoryMembership {
    /// Seed from an iterator of DIDs granted full read and write access.
    pub fn new<I: IntoIterator<Item = String>>(dids: I) -> Self {
        Self {
            members: RwLock::new(
                dids.into_iter()
                    .map(|did| (did, MemberAccess::READ_WRITE))
                    .collect(),
            ),
        }
    }
    /// Seed from an iterator of `(did, access)` pairs.
    pub fn with_access<I: IntoIterator<Item = (String, MemberAccess)>>(entries: I) -> Self {
        Self {
            members: RwLock::new(entries.into_iter().collect()),
        }
    }
    /// Add a member or replace their access, `putMember` semantics.
    pub fn set(&self, did: &str, access: MemberAccess) {
        self.members
            .write()
            .unwrap()
            .insert(did.to_string(), access);
    }
    pub fn remove(&self, did: &str) {
        self.members.write().unwrap().remove(did);
    }
}

#[async_trait]
impl MembershipStore for InMemoryMembership {
    async fn member_access(&self, did: &str) -> Result<MemberAccess> {
        Ok(self
            .members
            .read()
            .unwrap()
            .get(did)
            .copied()
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn set_remove_membership() {
        let members = InMemoryMembership::default();
        assert_eq!(
            members.member_access("did:plc:user").await.unwrap(),
            MemberAccess::NONE
        );
        members.set(
            "did:plc:user",
            MemberAccess {
                read: true,
                write: false,
            },
        );
        assert_eq!(
            members.member_access("did:plc:user").await.unwrap(),
            MemberAccess {
                read: true,
                write: false
            }
        );
        members.remove("did:plc:user");
        assert_eq!(
            members.member_access("did:plc:user").await.unwrap(),
            MemberAccess::NONE
        );
    }
}
