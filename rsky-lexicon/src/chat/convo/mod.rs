use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteMessageForSelfInput {
    pub convo_id: String,
    pub message_id: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
#[serde(rename = "chat.bsky.convo.defs#deletedMessageView")]
#[serde(rename_all = "camelCase")]
pub struct DeletedMessageView {
    pub id: String,
    pub rev: String,
    pub sender: String,
    pub sent_at: DateTime<Utc>
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageViewSender {
    pub did: String,
}