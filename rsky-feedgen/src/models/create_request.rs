#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "$type")]
pub enum Lexicon {
    #[serde(rename(deserialize = "app.bsky.feed.post", serialize = "app.bsky.feed.post"))]
    AppBskyFeedPost(rsky_lexicon::app::bsky::feed::Post),
    #[serde(rename(deserialize = "app.bsky.feed.like", serialize = "app.bsky.feed.like"))]
    AppBskyFeedLike(rsky_lexicon::app::bsky::feed::like::Like),
    #[serde(rename(
        deserialize = "app.bsky.graph.follow",
        serialize = "app.bsky.graph.follow"
    ))]
    AppBskyFeedFollow(rsky_lexicon::app::bsky::graph::follow::Follow),
}

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
    pub record: Lexicon,
}
