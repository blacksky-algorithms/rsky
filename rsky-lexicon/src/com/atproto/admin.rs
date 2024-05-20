use serde::{Deserialize, Serialize};

/// Delete a user account as an administrator.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DeleteAccountInput {
    pub did: String,
}

/// Disable an account from receiving new invite codes, but does not invalidate existing codes.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DisableAccountInvitesInput {
    pub account: String,
    /// Optional reason for disabled invites.
    pub note: Option<String>,
}

/// Disable some set of codes and/or all codes associated with a set of users.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DisableInviteCodesInput {
    pub codes: Option<Vec<String>>,
    pub accounts: Option<Vec<String>>,
}

/// Re-enable an account's ability to receive invite codes.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct EnableAccountInvitesInput {
    pub account: String,
    /// Optional reason for enabled invites.
    pub note: Option<String>,
}
