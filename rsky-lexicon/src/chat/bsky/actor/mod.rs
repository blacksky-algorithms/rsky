use crate::app::bsky::actor::{RefProfileAssociated, ViewerState};
use crate::com::atproto::label::Label;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileViewBasic {
    pub did: String,
    pub handle: String,
    pub display_name: Option<String>,
    pub avatar: Option<String>,
    pub associated: Option<RefProfileAssociated>,
    pub viewer: Option<ViewerState>,
    pub labels: Option<Vec<Label>>,
    // Set to true when the actor cannot actively participate in converations
    pub chat_disabled: Option<bool>,
}
