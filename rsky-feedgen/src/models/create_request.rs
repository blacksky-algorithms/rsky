#[derive(Debug, Serialize, Deserialize)]
pub struct CreateRequest {
    #[serde(rename = "uri")]
    pub uri: String,
    #[serde(rename = "cid")]
    pub cid: String,
    #[serde(rename = "sequence")]
    pub sequence: Option<i64>,
    #[serde(rename = "prev")]
    pub prev: Option<String>,
    #[serde(rename = "author")]
    pub author: String,
    #[serde(rename = "record")]
    pub record: lexicon::app::bsky::feed::Post,
}
