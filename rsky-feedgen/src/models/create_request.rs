#[derive(Debug, Serialize, Deserialize)]
pub struct CreateRequest {
    #[serde(rename = "uri")]
    pub uri: String,
    #[serde(rename = "cid")]
    pub cid: String,
    #[serde(rename = "author")]
    pub author: String,
    #[serde(rename = "record")]
    pub record: lexicon::app::bsky::feed::Post,
}