use serde::{Deserialize, Serialize};

/// Delete a user account as an administrator.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DeleteAccountInput {
    pub did: String,
}

/// Disable an account from receiving new invite codes, but does not invalidate existing codes.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DisableAccountInvites {
    pub account: String,
    /// Optional reason for disabled invites.
    pub note: Option<String>,
}
