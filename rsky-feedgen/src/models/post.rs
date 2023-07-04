use crate::schema::post;
use diesel::backend::Backend;
use diesel::deserialize::{self, Queryable};
use diesel::prelude::Selectable;

type DB = diesel::pg::Pg;

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Post {
    #[serde(rename = "uri")]
    pub uri: String,
    #[serde(rename = "cid")]
    pub cid: String,
    #[serde(rename = "replyParent", skip_serializing_if = "Option::is_none")]
    pub reply_parent: Option<String>,
    #[serde(rename = "replyRoot", skip_serializing_if = "Option::is_none")]
    pub reply_root: Option<String>,
    #[serde(rename = "indexedAt")]
    pub indexed_at: String,
    #[serde(rename = "prev", skip_serializing_if = "Option::is_none")]
    pub prev: Option<String>,
    #[serde(rename = "sequence")]
    pub sequence: Option<i64>,
}

impl Queryable<post::SqlType, DB> for Post {
    type Row = (
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        Option<i64>,
    );

    fn build(row: Self::Row) -> deserialize::Result<Self> {
        Ok(Post {
            uri: row.0,
            cid: row.1,
            reply_parent: row.2,
            reply_root: row.3,
            indexed_at: row.4,
            prev: row.5,
            sequence: row.6,
        })
    }
}

impl<DB> Selectable<DB> for Post
where
    DB: Backend,
{
    type SelectExpression = (
        post::uri,
        post::cid,
        post::replyParent,
        post::replyRoot,
        post::indexedAt,
        post::prev,
        post::sequence,
    );

    fn construct_selection() -> Self::SelectExpression {
        (
            post::uri,
            post::cid,
            post::replyParent,
            post::replyRoot,
            post::indexedAt,
            post::prev,
            post::sequence,
        )
    }
}
