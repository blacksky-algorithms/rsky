use super::SimpleStore;
use super::memory::MemorySimpleStore;
use jose_jwk::Key;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InternalStateData {
    pub iss: String,
    pub dpop_key: Key,
    pub verifier: String,
}

pub trait StateStore: SimpleStore<String, InternalStateData> {}

pub type MemoryStateStore = MemorySimpleStore<String, InternalStateData>;

impl StateStore for MemoryStateStore {}
