use crate::app::bsky::actor::ProfileViewBasic;
use crate::app::bsky::embed::EmbedViews;
use crate::app::bsky::feed::{BlockedAuthor, GeneratorView};
use crate::app::bsky::graph::{ListView, StarterPackViewBasic};
use crate::app::bsky::labeler::LabelerView;
use crate::com::atproto::label::Label;
use crate::com::atproto::repo::StrongRef;
use serde_json::Value;

/// A representation of a record embedded in a Bluesky record (eg, a post).
/// For example, a quote-post, or sharing a feed generator record.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Record {
    pub record: StrongRef,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
#[serde(rename = "app.bsky.embed.record#viewRecord")]
#[serde(rename_all = "camelCase")]
pub struct ViewRecord {
    pub uri: String,
    pub cid: String,
    pub author: ProfileViewBasic,
    pub value: Value,
    pub labels: Option<Vec<Label>>,
    pub reply_count: Option<usize>,
    pub repost_count: Option<usize>,
    pub like_count: Option<usize>,
    pub embeds: Option<Vec<EmbedViews>>,
    pub indexed_at: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
#[serde(rename = "app.bsky.embed.record#view")]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub record: ViewUnion,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ViewUnion {
    ViewRecord(ViewRecord),
    ViewNotFound(ViewNotFound),
    ViewBlocked(ViewBlocked),
    GeneratorView(GeneratorView),
    ListView(ListView),
    LabelerView(LabelerView),
    StarterPackViewBasic(StarterPackViewBasic),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
#[serde(rename = "app.bsky.embed.record#viewNotFound")]
#[serde(rename_all = "camelCase")]
pub struct ViewNotFound {
    pub uri: String,
    pub not_found: bool,
}

impl Default for ViewNotFound {
    fn default() -> Self {
        Self {
            uri: "".to_string(),
            not_found: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
#[serde(rename = "app.bsky.embed.record#viewBlocked")]
#[serde(rename_all = "camelCase")]
pub struct ViewBlocked {
    pub uri: String,
    pub blocked: bool,
    pub author: BlockedAuthor,
}
