use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct InviteCode {
    pub code: String,
    pub available_uses: i32,
    pub disabled: bool,
    pub for_account: String,
    pub created_by: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateInviteCodeInput {
    pub use_count: i32,
    pub for_account: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateInviteCodeOutput {
    pub code: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateInviteCodesInput {
    pub code_count: i32,
    pub use_count: i32,
    pub for_accounts: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateInviteCodesOutput {
    pub codes: Vec<AccountCodes>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AccountCodes {
    pub account: String,
    pub codes: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GetAccountInviteCodesOutput {
    pub codes: Vec<InviteCode>,
}