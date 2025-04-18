use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum AccessTokenType {
    AUTO,
    JWT,
    ID,
    w,
}
