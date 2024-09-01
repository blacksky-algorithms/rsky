pub mod follow;

use crate::app::bsky::actor::{ProfileView, ProfileViewBasic};
use crate::app::bsky::richtext::Facet;
use crate::com::atproto::label::Label;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListViewBasic {
    pub uri: String,
    pub cid: String,
    pub name: String,
    pub purpose: ListPurpose,
    pub avatar: Option<String>,
    pub list_item_count: Option<usize>,
    pub labels: Option<Vec<Label>>,
    pub viewer: Option<ListViewerState>,
    pub indexed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
#[serde(rename = "app.bsky.graph.defs#listView")]
#[serde(rename_all = "camelCase")]
pub struct ListView {
    pub uri: String,
    pub cid: String,
    pub creator: ProfileView,
    pub name: String,
    pub purpose: ListPurpose,
    pub description: Option<String>,
    pub description_facets: Option<Vec<Facet>>,
    pub avatar: Option<String>,
    pub list_item_count: Option<usize>,
    pub labels: Option<Vec<Label>>,
    pub viewer: Option<ListViewerState>,
    pub indexed_at: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
#[serde(rename = "app.bsky.graph.defs#starterPackViewBasic")]
#[serde(rename_all = "camelCase")]
pub struct StarterPackViewBasic {
    pub uri: String,
    pub cid: String,
    pub record: Value,
    pub creator: ProfileViewBasic,
    pub list_item_count: Option<usize>,
    pub joined_week_count: Option<usize>,
    pub joined_all_time_count: Option<usize>,
    pub labels: Option<Vec<Label>>,
    pub indexed_at: String,
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
