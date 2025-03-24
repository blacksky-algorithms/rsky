#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct AlgoResponse {
    #[serde(rename = "cursor", skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(rename = "feed")]
    pub feed: Vec<crate::models::PostResult>,
}
