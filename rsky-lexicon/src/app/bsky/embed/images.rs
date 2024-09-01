use crate::com::atproto::repo::Blob;

/// A set of images embedded in a Bluesky record (eg, a post).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Images {
    pub images: Vec<Image>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Image {
    pub image: Blob,
    /// Alt text description of the image, for accessibility
    pub alt: String,
    pub aspect_ratio: Option<AspectRatio>,
}

/// width:height represents an aspect ratio. It may be approximate,
/// and may not correspond to absolute dimensions in any given unit.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct AspectRatio {
    pub width: usize,
    pub height: usize,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
#[serde(rename = "app.bsky.embed.images#view")]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub images: Vec<ViewImage>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewImage {
    /// Fully-qualified URL where a thumbnail of the image can be fetched.
    /// For example, CDN location provided by the App View.
    pub thumb: String,
    /// Fully-qualified URL where a large version of the image can be fetched.
    /// May or may not be the exact original blob. For example, CDN location provided by the App View.
    pub fullsize: String,
    /// Alt text description of the image, for accessibility.
    pub alt: String,
    pub aspect_ratio: Option<AspectRatio>,
}
