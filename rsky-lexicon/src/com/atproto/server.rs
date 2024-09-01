use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateInviteCodeInput {
    #[serde(rename(deserialize = "useCount", serialize = "useCount"))]
    pub use_count: i32,
    #[serde(rename(deserialize = "forAccount", serialize = "forAccount"))]
    pub for_account: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateInviteCodesInput {
    #[serde(rename(deserialize = "codeCount", serialize = "codeCount"))]
    pub code_count: i32,
    #[serde(rename(deserialize = "useCount", serialize = "useCount"))]
    pub use_count: i32,
    #[serde(rename(deserialize = "forAccounts", serialize = "forAccounts"))]
    pub for_accounts: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AccountCodes {
    pub account: String,
    pub codes: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CreateAccountInput {
    pub email: Option<String>,
    pub handle: String,
    pub did: Option<String>,
    #[serde(rename(deserialize = "inviteCode", serialize = "inviteCode"))]
    pub invite_code: Option<String>,
    #[serde(rename(deserialize = "verificationCode", serialize = "verificationCode"))]
    pub verification_code: Option<String>,
    #[serde(rename(deserialize = "verificationPhone", serialize = "verificationPhone"))]
    pub verification_phone: Option<String>,
    pub password: Option<String>,
    #[serde(rename(deserialize = "recoveryKey", serialize = "recoveryKey"))]
    pub recovery_key: Option<String>,
    #[serde(rename(deserialize = "plcOp", serialize = "plcOp"))]
    pub plc_op: Option<String>,
}

/// Create an App Password
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CreateAppPasswordInput {
    /// A short name for the App Password, to help distinguish them.
    pub name: String,
}

/// Create an authentication session.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CreateSessionInput {
    /// Handle or other identifier supported by the server for the authenticating user.
    pub identifier: String,
    pub password: String,
}

/// Delete an actor's account with a token and password. Can only be called after
/// requesting a deletion token. Requires auth
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DeleteAccountInput {
    pub did: String,
    pub password: String,
    pub token: String,
}

/// Confirm an email using a token from com.atproto.server.requestEmailConfirmation.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ConfirmEmailInput {
    pub email: String,
    pub token: String,
}

/// Deactivates a currently active account. Stops serving of repo, and future writes to repo
/// until reactivated. Used to finalize account migration with the old host
/// after the account has been activated on the new host.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DeactivateAccountInput {
    /// A recommendation to server as to how long they should hold onto the deactivated account
    /// before deleting.
    #[serde(rename = "deleteAfter")]
    pub delete_after: Option<String>,
}

/// Initiate a user account password reset via email.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RequestPasswordResetInput {
    pub email: String,
}

/// Reset a user account password using a token.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ResetPasswordInput {
    pub token: String,
    pub password: String,
}

/// Revoke an App Password by name.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RevokeAppPasswordInput {
    pub name: String,
}

/// Update an account's email.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UpdateEmailInput {
    pub email: String,
    /// Requires a token from com.atproto.sever.requestEmailUpdate
    /// if the account's email has been confirmed.
    pub token: Option<String>,
}

// Outputs
// -------

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateInviteCodeOutput {
    pub code: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateInviteCodesOutput {
    pub codes: Vec<AccountCodes>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GetAccountInviteCodesOutput {
    pub codes: Vec<InviteCode>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CreateAppPasswordOutput {
    pub name: String,
    pub password: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateAccountOutput {
    pub handle: String,
    pub did: String,
    #[serde(rename = "didDoc", skip_serializing_if = "Option::is_none")]
    pub did_doc: Option<Value>,
    #[serde(rename = "accessJwt")]
    pub access_jwt: String,
    #[serde(rename = "refreshJwt")]
    pub refresh_jwt: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CreateSessionOutput {
    #[serde(rename = "accessJwt")]
    pub access_jwt: String,
    #[serde(rename = "refreshJwt")]
    pub refresh_jwt: String,
    pub handle: String,
    pub did: String,
    #[serde(rename = "didDoc", skip_serializing_if = "Option::is_none")]
    pub did_doc: Option<String>,
    pub email: Option<String>,
    #[serde(rename = "emailConfirmed", skip_serializing_if = "Option::is_none")]
    pub email_confirmed: Option<bool>,
}

/// Get information about the current auth session. Requires auth.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GetSessionOutput {
    pub handle: String,
    pub did: String,
    pub email: Option<String>,
    #[serde(rename = "emailConfirmed", skip_serializing_if = "Option::is_none")]
    pub email_confirmed: Option<bool>,
    #[serde(rename = "didDoc", skip_serializing_if = "Option::is_none")]
    pub did_doc: Option<String>,
}

/// Describes the server's account creation requirements and capabilities. Implemented by PDS.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DescribeServerOutput {
    /// If true, an invite code must be supplied to create an account on this instance.
    #[serde(rename = "inviteCodeRequired", skip_serializing_if = "Option::is_none")]
    pub invite_code_required: Option<bool>,
    /// If true, a phone verification token must be supplied to create an account on this instance.
    #[serde(
        rename = "phoneVerificationRequired",
        skip_serializing_if = "Option::is_none"
    )]
    pub phone_verification_required: Option<bool>,
    /// List of domain suffixes that can be used in account handles..
    #[serde(rename = "availableUserDomains")]
    pub available_user_domains: Vec<String>,
    /// URLs of service policy documents.
    pub links: DescribeServerRefLinks,
    /// Contact information
    pub contact: DescribeServerRefContact,
    pub did: String,
}

/// Get a signed token on behalf of the requesting DID for the requested service.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GetServiceAuthOutput {
    pub token: String,
}

/// Returns the status of an account, especially as pertaining to import or recovery.
/// Can be called many times over the course of an account migration. Requires auth and
/// can only be called pertaining to oneself.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CheckAccountStatusOutput {
    pub activated: bool,
    #[serde(rename = "validDid")]
    pub valid_did: bool,
    #[serde(rename = "repoCommit")]
    pub repo_commit: String,
    #[serde(rename = "repoRev")]
    pub repo_rev: String,
    #[serde(rename = "repoBlocks")]
    pub repo_blocks: i64,
    #[serde(rename = "indexedRecords")]
    pub indexed_records: i64,
    #[serde(rename = "privateStateValues")]
    pub private_state_values: i64,
    #[serde(rename = "expectedBlobs")]
    pub expected_blobs: i64,
    #[serde(rename = "importedBlobs")]
    pub imported_blobs: i64,
}

/// List all App Passwords.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ListAppPasswordsOutput {
    pub passwords: Vec<AppPassword>,
}

/// Refresh an authentication session. Requires auth using the 'refreshJwt' (not the 'accessJwt').
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RefreshSessionOutput {
    pub handle: String,
    pub did: String,
    #[serde(rename = "didDoc", skip_serializing_if = "Option::is_none")]
    pub did_doc: Option<String>,
    #[serde(rename = "accessJwt")]
    pub access_jwt: String,
    #[serde(rename = "refreshJwt")]
    pub refresh_jwt: String,
}

/// Request a token in order to update email.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RequestEmailUpdateOutput {
    #[serde(rename = "tokenRequired")]
    pub token_required: bool,
}

// Defs
// ----

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InviteCode {
    pub code: String,
    pub available: i32,
    pub disabled: bool,
    #[serde(rename(deserialize = "forAccount", serialize = "forAccount"))]
    pub for_account: String,
    #[serde(rename(deserialize = "createdBy", serialize = "createdBy"))]
    pub created_by: String,
    #[serde(rename(deserialize = "createdAt", serialize = "createdAt"))]
    pub created_at: String,
    pub uses: Vec<InviteCodeUse>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InviteCodeUse {
    #[serde(rename(deserialize = "usedBy", serialize = "usedBy"))]
    pub used_by: String,
    #[serde(rename(deserialize = "usedAt", serialize = "usedAt"))]
    pub used_at: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DescribeServerRefLinks {
    #[serde(rename = "privacyPolicy", skip_serializing_if = "Option::is_none")]
    pub privacy_policy: Option<String>,
    #[serde(rename = "termsOfService", skip_serializing_if = "Option::is_none")]
    pub terms_of_service: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DescribeServerRefContact {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AppPassword {
    pub name: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}
