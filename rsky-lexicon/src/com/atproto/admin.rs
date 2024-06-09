use crate::com::atproto::repo::StrongRef;
use crate::com::atproto::server::InviteCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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

/// Administrative action to update an account's email.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UpdateAccountEmailInput {
    /// The handle or DID of the repo.
    pub account: String,
    pub email: String,
}

/// Administrative action to update an account's handle.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UpdateAccountHandleInput {
    pub did: String,
    pub handle: String,
}

/// Update the password for a user account as an administrator.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UpdateAccountPasswordInput {
    pub did: String,
    pub password: String,
}

/// Send email to a user's account email address.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SendMailInput {
    #[serde(rename = "recipientDid")]
    pub recipient_did: String,
    pub content: String,
    pub subject: Option<String>,
    #[serde(rename = "senderDid")]
    pub sender_did: String,
    /// Additional comment by the sender that won't be used in the email itself but helpful to
    /// provide more context for moderators/reviewers
    pub comment: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SendMailOutput {
    pub sent: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct GetInviteCodesOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub codes: Vec<InviteCode>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SubjectStatus {
    pub subject: Subject,
    pub takedown: Option<StatusAttr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deactivated: Option<StatusAttr>,
}

#[derive(Debug, Serialize)]
pub struct UpdateSubjectStatusOutput {
    pub subject: Subject,
    pub takedown: Option<StatusAttr>,
}

// Defs
// ----

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AccountView {
    pub did: String,
    pub handle: String,
    pub email: Option<String>,
    #[serde(rename = "relatedRecords")]
    pub related_records: Option<Vec<Value>>,
    #[serde(rename = "indexedAt")]
    pub indexed_at: String,
    #[serde(rename = "invitedBy")]
    pub invited_by: Option<InviteCode>,
    pub invites: Option<Vec<InviteCode>>,
    #[serde(rename = "invitesDisabled")]
    pub invites_disabled: Option<bool>,
    #[serde(rename = "emailConfirmedAt")]
    pub email_confirmed_at: Option<String>,
    #[serde(rename = "inviteNote")]
    pub invite_note: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct StatusAttr {
    pub applied: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "$type")]
pub enum Subject {
    #[serde(rename = "com.atproto.admin.defs#repoRef")]
    RepoRef(RepoRef),
    #[serde(rename = "com.atproto.repo.strongRef")]
    StrongRef(StrongRef),
    #[serde(rename = "com.atproto.admin.defs#repoBlobRef")]
    RepoBlobRef(RepoBlobRef),
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RepoRef {
    pub did: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RepoBlobRef {
    pub did: String,
    pub cid: String,
    #[serde(rename = "recordUri")]
    pub record_uri: Option<String>,
}
