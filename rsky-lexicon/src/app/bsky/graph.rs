use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Follow {
    #[serde(rename(deserialize = "$type", serialize = "$type"))]
    pub rust_type: Option<String>,
    #[serde(rename(deserialize = "createdAt"))]
    #[serde(rename(serialize = "createdAt"))]
    pub created_at: String,
    pub subject: String,
}
