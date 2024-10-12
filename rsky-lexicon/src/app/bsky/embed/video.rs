use crate::app::bsky::embed::images::AspectRatio;
use crate::com::atproto::repo::Blob;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Video {
    pub video: Blob,
    pub captions: Option<Vec<Caption>>,
    /// Alt text description of video image, for accessibility
    pub alt: Option<String>,
    pub aspect_ratio: Option<AspectRatio>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Caption {
    pub lang: String,
    pub file: Blob,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
#[serde(rename = "app.bsky.embed.video#view")]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub cid: String,
    pub playlist: String,
    pub thumbnail: Option<String>,
    pub alt: Option<String>,
    pub aspect_ratio: Option<AspectRatio>,
}
