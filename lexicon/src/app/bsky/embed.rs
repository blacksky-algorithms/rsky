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

#[derive(Debug, Deserialize, Serialize)]
pub struct Image {
    pub image: Blob,
    pub alt: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ViewImage {
    pub thumb: String,
    #[serde(rename(deserialize = "fullSize", serialize = "fullSize"))]
    pub full_size: String,
    pub alt: String,
}

///app.bsky.embed.external#external
#[derive(Debug, Deserialize, Serialize)]
pub struct ExternalObject {
    pub uri: String,
    pub title: String,
    pub description: String,
    #[serde(rename(deserialize = "maxSize", serialize = "maxSize"))]
    pub max_size: Option<usize>,
}

///app.bsky.embed.external
#[derive(Debug, Deserialize, Serialize)]
pub struct External {
    pub external: ExternalObject,
}
