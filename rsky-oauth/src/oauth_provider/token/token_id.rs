use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenId(String);

impl TokenId {
    pub fn new(val: impl Into<String>) -> Self {
        TokenId(val.into())
    }

    pub fn val(&self) -> String {
        self.0.clone()
    }
}

pub enum TokenIdError {
    InvalidLength,
    InvalidFormat(String),
}

pub fn is_token_id(data: &str) -> bool {
    let prefix = &data.to_string()[..4];
    prefix == "tok-"
}
