use crate::app::bsky::embed::record::View as EmbedRecordView;
use crate::app::bsky::richtext::Facet;
use crate::chat::bsky::actor::ProfileViewBasic;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
#[serde(rename = "chat.bsky.convo.defs#messageView")]
#[serde(rename_all = "camelCase")]
pub struct MessageView {
    pub id: String,
    pub rev: String,
    pub text: String,
    // Annotations of text (mentions, URLs, hashtags, etc)
    pub facets: Option<Vec<Facet>>,
    pub embed: Option<EmbedRecordView>,
    pub sender: String,
    pub sent_at: DateTime<Utc>,
}

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
    pub sender: MessageViewSender,
    pub sent_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageViewSender {
    pub did: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
#[serde(rename = "chat.bsky.convo.defs#convoView")]
#[serde(rename_all = "camelCase")]
pub struct ConvoView {
    pub id: String,
    pub rev: String,
    pub members: Vec<ProfileViewBasic>,
    pub last_message: Option<LastMessageEnum>,
    pub muted: bool,
    pub unread_count: u64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum LastMessageEnum {
    MessageView(MessageView),
    DeletedMessageView(DeletedMessageView),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetConvoOutput {
    pub convo: ConvoView,
}
