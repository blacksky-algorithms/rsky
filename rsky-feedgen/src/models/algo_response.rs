#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct AlgoResponse {
    #[serde(rename = "cursor")]
    pub cursor: Option<String>,
    #[serde(rename = "feed")]
    pub feed: Vec<crate::models::PostResult>,
}
