use crate::app::bsky::embed::record::{Record, View as EmbedRecordView};
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
    pub last_message: Option<MessageViewEnum>,
    pub muted: bool,
    pub unread_count: u64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum MessageViewEnum {
    MessageView(MessageView),
    DeletedMessageView(DeletedMessageView),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetConvoOutput {
    pub convo: ConvoView,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
#[serde(rename = "chat.bsky.convo.defs#logBeginConvo")]
#[serde(rename_all = "camelCase")]
pub struct LogBeginConvo {
    pub rev: String,
    pub convo_id: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
#[serde(rename = "chat.bsky.convo.defs#logLeaveConvo")]
#[serde(rename_all = "camelCase")]
pub struct LogLeaveConvo {
    pub rev: String,
    pub convo_id: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
#[serde(rename = "chat.bsky.convo.defs#logCreateMessage")]
#[serde(rename_all = "camelCase")]
pub struct LogCreateMessage {
    pub rev: String,
    pub convo_id: String,
    pub message: MessageViewEnum,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
#[serde(rename = "chat.bsky.convo.defs#logDeleteMessage")]
#[serde(rename_all = "camelCase")]
pub struct LogDeleteMessage {
    pub rev: String,
    pub convo_id: String,
    pub message: MessageViewEnum,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum LogEnum {
    LogBeginConvo(LogBeginConvo),
    LogLeaveConvo(LogLeaveConvo),
    LogCreateMessage(LogCreateMessage),
    LogDeleteMessage(LogDeleteMessage),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetLogOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub logs: Vec<LogEnum>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetMessagesOutput {
    pub messages: Vec<MessageViewEnum>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaveConvoInput {
    pub convo_id: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaveConvoOutput {
    pub convo_id: String,
    pub rev: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListConvosOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub convos: Vec<ConvoView>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MuteConvoInput {
    pub convo_id: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MuteConvoOutput {
    pub convo: ConvoView,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageInput {
    pub convo_id: String,
    pub message: MessageInput,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageInput {
    pub text: String,
    // Annotations of text (mentions, URLs, hashtags, etc)
    pub facets: Option<Vec<Facet>>,
    pub embed: Option<Record>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageBatchInput {
    pub items: Vec<BatchItem>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageBatchOutput {
    pub items: Vec<MessageView>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchItem {
    pub convo_id: String,
    pub message: MessageInput,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnmuteConvoInput {
    pub convo_id: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnmuteConvoOutput {
    pub convo: ConvoView,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateReadInput {
    pub convo_id: String,
    pub message_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateReadOutput {
    pub convo: ConvoView,
}
