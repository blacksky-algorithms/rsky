#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct DeleteOp {
    #[serde(rename = "uri")]
    pub uri: String,
}
