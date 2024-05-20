use serde::{Deserialize, Serialize};

/// Delete a user account as an administrator.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DeleteAccountInput {
    pub did: String,
}
