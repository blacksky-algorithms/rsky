use libipld::Cid;
use rsky_lexicon::app::bsky::actor::Profile;
use rsky_lexicon::app::bsky::feed::Post;
use rsky_syntax::aturi::AtUri;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct LocalRecords {
    pub count: i64,
    pub profile: Option<RecordDescript<Profile>>,
    pub posts: Vec<RecordDescript<Post>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RecordDescript<T> {
    pub uri: AtUri,
    pub cid: Cid,
    #[serde(rename = "indexedAt")]
    pub indexed_at: String,
    pub record: T,
}
