use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct InviteCode {
    pub code: String,
    #[serde(rename(deserialize = "availableUses", serialize = "availableUses"))]
    pub available_uses: i32,
    pub disabled: bool,
    #[serde(rename(deserialize = "forAccount", serialize = "forAccount"))]
    pub for_account: String,
    #[serde(rename(deserialize = "createdBy", serialize = "createdBy"))]
    pub created_by: String,
    #[serde(rename(deserialize = "createdAt", serialize = "createdAt"))]
    pub created_at: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateInviteCodeInput {
    #[serde(rename(deserialize = "useCount", serialize = "useCount"))]
    pub use_count: i32,
    #[serde(rename(deserialize = "forAccount", serialize = "forAccount"))]
    pub for_account: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateInviteCodeOutput {
    pub code: String,
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

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateInviteCodesOutput {
    pub codes: Vec<AccountCodes>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AccountCodes {
    pub account: String,
    pub codes: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GetAccountInviteCodesOutput {
    pub codes: Vec<InviteCode>,
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
    pub did_doc: Option<String>,
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
