//! Template structs, one file per screen family.

pub mod account;
pub mod oauth;

/// An account as the picker, consent, and account pages show it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccountCardView {
    pub did: String,
    pub handle: String,
    /// Display name when one is known; empty otherwise
    pub name: String,
    /// Avatar URL when one is known; empty otherwise
    pub picture: String,
    pub login_required: bool,
    pub deactivated: bool,
}

impl AccountCardView {
    pub fn from_account(account: &rsky_oauth::store::AccountInfo, login_required: bool) -> Self {
        AccountCardView {
            did: account.did.clone(),
            handle: account
                .handle
                .clone()
                .map(|h| format!("@{h}"))
                .unwrap_or_else(|| account.did.clone()),
            name: String::new(),
            picture: String::new(),
            login_required,
            deactivated: account.deactivated,
        }
    }
}
