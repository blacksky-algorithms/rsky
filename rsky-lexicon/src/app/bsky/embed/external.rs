use crate::com::atproto::repo::Blob;

/// A representation of some externally linked content (eg, a URL and 'card'),
/// embedded in a Bluesky record (eg, a post).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct External {
    pub external: ExternalObject,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalObject {
    pub uri: String,
    pub title: String,
    pub description: String,
    pub thumb: Option<Blob>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
#[serde(rename = "app.bsky.embed.external#view")]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub external: ViewExternal,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewExternal {
    pub uri: String,
    pub title: String,
    pub description: String,
    pub thumb: Option<String>,
}
