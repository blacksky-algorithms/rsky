#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct CreateOp<T> {
    #[serde(rename = "uri")]
    pub uri: String,
    #[serde(rename = "cid")]
    pub cid: String,
    #[serde(rename = "sequence")]
    pub sequence: i64,
    #[serde(rename = "prev")]
    pub prev: Option<String>,
    #[serde(rename = "author")]
    pub author: String,
    #[serde(rename = "record")]
    pub record: T,
}
