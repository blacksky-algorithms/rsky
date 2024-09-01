use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ResolveHandleOutput {
    pub did: String,
}

/// Updates the current account's handle. Verifies handle validity, and updates did:plc document if
/// necessary. Implemented by PDS, and requires auth.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UpdateHandleInput {
    /// The new handle.
    pub handle: String,
}
