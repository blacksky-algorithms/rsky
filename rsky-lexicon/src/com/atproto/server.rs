use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct InviteCode {
    pub code: String,
    pub available_uses: u8,
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
