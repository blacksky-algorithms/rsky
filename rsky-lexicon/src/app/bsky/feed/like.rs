use crate::com::atproto::repo::StrongRef;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
#[serde(rename = "app.bsky.feed.like")]
#[serde(rename_all = "camelCase")]
pub struct Like {
    pub created_at: DateTime<Utc>,
    pub subject: StrongRef,
}
