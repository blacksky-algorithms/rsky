//! per-repo app-owned state in storage, colocated with hubble-sync's info
//!
//! "si|"||<DID>||NUL||"u" => <bytes>
//!
//! that is,
//!
//! <AccountSyncState::key(&Did)>||NUL||"u" => <bytes>

use super::prev::AccountSyncState;
use super::{DecodeError, LoadError};
use crate::{Did, RepoSlot, Slot, StorageBatch, StorageEngine, StorageError};

#[derive(Debug)]
pub struct PrevSlot;

impl PrevSlot {
    pub fn key(did: &Did) -> Vec<u8> {
        let mut k = AccountSyncState::key(did);
        k.extend_from_slice(&[0x00, b'u']);
        k
    }

    pub fn load<S: StorageEngine, T: RepoSlot>(
        did: &Did,
        storage: &S,
    ) -> Result<Option<T>, LoadError<S::Error>> {
        if !T::PRESENT {
            return Ok(None);
        }
        let Some(bytes) = storage.get(&Self::key(did)).map_err(LoadError::Storage)? else {
            return Ok(None);
        };
        let t = T::decode(&bytes).map_err(|e| DecodeError::RepoSlotDecode {
            did: did.clone(),
            slot: Slot::Info,
            err: e,
        })?;
        Ok(Some(t))
    }

    pub fn store<E: StorageError, B: StorageBatch<E>, T: RepoSlot>(
        did: &Did,
        value: &T,
        batch: &mut B,
    ) {
        if !T::PRESENT {
            return;
        }
        batch.put(&Self::key(did), &value.encode());
    }

    pub fn delete<E: StorageError, B: StorageBatch<E>>(did: &Did, batch: &mut B) {
        batch.delete(&Self::key(did));
    }
}
