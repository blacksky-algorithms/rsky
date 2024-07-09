use crate::com::atproto::repo::Blob;
use serde::{Deserialize, Serialize};

// "app.bsky.embed.images#view",
// "app.bsky.embed.external#view",
// "app.bsky.embed.record#view",
// "app.bsky.embed.recordWithMedia#view"

///app.bsky.embed.images
// #[derive(Debug, Deserialize, Serialize)]
// pub struct Images {
//     pub images: Vec<Image>,
// }

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ImagesEmbed {
    pub images: Vec<Image>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Record {
    pub uri: String,
    pub cid: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RecordEmbed {
    pub record: Record,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
pub enum Media {
    #[serde(rename = "app.bsky.embed.images")]
    Images(ImagesEmbed),

    #[serde(rename = "app.bsky.embed.external")]
    External(External),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RecordWithMedia {
    pub record: RecordEmbed,
    pub media: Media,
}

// "app.bsky.embed.images",
// "app.bsky.embed.external",
// "app.bsky.embed.record",
// "app.bsky.embed.recordWithMedia"
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
pub enum Embeds {
    #[serde(rename = "app.bsky.embed.images")]
    Images(ImagesEmbed),

    #[serde(
        alias = "app.bsky.embed.external",
        alias = "app.bsky.embed.external#main"
    )]
    External(External),

    #[serde(rename = "app.bsky.embed.record")]
    Record(RecordEmbed),

    #[serde(rename = "app.bsky.embed.recordWithMedia")]
    RecordWithMedia(RecordWithMedia),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Image {
    pub image: Blob,
    pub alt: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ViewImage {
    pub thumb: String,
    #[serde(rename(deserialize = "fullSize", serialize = "fullSize"))]
    pub full_size: String,
    pub alt: String,
}

///app.bsky.embed.external#external
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ExternalObject {
    pub uri: String,
    pub title: String,
    pub description: String,
    #[serde(rename(deserialize = "maxSize", serialize = "maxSize"))]
    pub max_size: Option<usize>,
    pub thumb: Option<Blob>,
}

///app.bsky.embed.external
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct External {
    pub external: ExternalObject,
}
