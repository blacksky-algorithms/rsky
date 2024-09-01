use crate::app::bsky::embed::record::{Record, View as ViewRecord};
use crate::app::bsky::embed::{MediaUnion, MediaViewUnion};

/// A representation of a record embedded in a Bluesky record (eg, a post),
/// alongside other compatible embeds. For example, a quote post and image,
/// or a quote post and external URL card.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordWithMedia {
    pub record: Record,
    pub media: MediaUnion,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
#[serde(rename = "app.bsky.embed.recordWithMedia#view")]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub record: ViewRecord,
    pub media: MediaViewUnion,
}
