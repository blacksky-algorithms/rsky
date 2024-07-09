use crate::com::atproto::label::Label;

/// Record declaring a social 'follow' relationship of another account. 
/// Duplicate follows will be ignored by the AppView.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
#[serde(rename = "app.bsky.graph.follow")]
#[serde(rename_all = "camelCase")]
pub struct Follow {
    pub created_at: String,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ListViewBasic {
    pub uri: String,
    pub cid: String,
    pub name: String,
    pub purpose: ListPurpose,
    pub avatar: Option<String>,
    #[serde(rename = "listItemCount")]
    pub list_item_count: Option<usize>,
    pub labels: Option<Vec<Label>>,
    pub viewer: Option<ListViewerState>,
    #[serde(rename = "indexedAt")]
    pub indexed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum ListPurpose {
    /// A list of actors to apply an aggregate moderation action (mute/block) on.
    #[serde(rename = "app.bsky.graph.defs#modlist")]
    ModList,
    /// A list of actors used for curation purposes such as list feeds or interaction gating.
    #[serde(rename = "app.bsky.graph.defs#curatelist")]
    CurateList,
    /// A list of actors used for only for reference purposes such as within a starter pack.
    #[serde(rename = "app.bsky.graph.defs#referencelist")]
    ReferenceList,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ListViewerState {
    pub muted: Option<bool>,
    pub blocked: Option<String>,
}
