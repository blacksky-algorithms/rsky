use crate::app::bsky::actor::ProfileViewBasic;
use crate::com::atproto::label::Label;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
#[serde(rename = "app.bsky.labeler.defs#labelerView")]
#[serde(rename_all = "camelCase")]
pub struct LabelerView {
    pub uri: String,
    pub cid: String,
    pub creator: ProfileViewBasic,
    pub like_count: Option<usize>,
    pub viewer: Option<LabelerViewerState>,
    pub indexed_at: String,
    pub labels: Option<Vec<Label>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct LabelerViewerState {
    pub like: Option<String>,
}
