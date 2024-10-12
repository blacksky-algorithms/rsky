pub mod external;
pub mod images;
pub mod record;
pub mod record_with_media;
pub mod video;

use crate::app::bsky::embed::external::{External, View as ExternalView};
use crate::app::bsky::embed::images::{Images, View as ImagesView};
use crate::app::bsky::embed::record::{Record, View as RecordView};
use crate::app::bsky::embed::record_with_media::{RecordWithMedia, View as RecordWithMediaView};
use crate::app::bsky::embed::video::{Video, View as VideoView};

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
pub enum MediaUnion {
    #[serde(rename = "app.bsky.embed.images")]
    Images(Images),
    #[serde(rename = "app.bsky.embed.video")]
    Video(Video),
    #[serde(rename = "app.bsky.embed.external")]
    External(External),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
pub enum MediaViewUnion {
    #[serde(rename = "app.bsky.embed.images#view")]
    ImagesView(ImagesView),
    #[serde(rename = "app.bsky.embed.video#view")]
    VideoView(VideoView),
    #[serde(rename = "app.bsky.embed.external#view")]
    ExternalView(ExternalView),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
pub enum Embeds {
    #[serde(rename = "app.bsky.embed.images")]
    Images(Images),

    #[serde(rename = "app.bsky.embed.video")]
    Video(Video),

    #[serde(
        alias = "app.bsky.embed.external",
        alias = "app.bsky.embed.external#main"
    )]
    External(External),

    #[serde(rename = "app.bsky.embed.record")]
    Record(Record),

    #[serde(rename = "app.bsky.embed.recordWithMedia")]
    RecordWithMedia(RecordWithMedia),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum EmbedViews {
    ImagesView(ImagesView),
    ExternalView(ExternalView),
    VideoView(VideoView),
    RecordView(RecordView),
    RecordWithMediaView(RecordWithMediaView),
}
