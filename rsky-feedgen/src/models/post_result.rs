#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct PostResult {
    #[serde(rename = "post")]
    pub post: String,
}
