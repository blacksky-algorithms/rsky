use crate::schema::post;
use diesel::backend::Backend;
use diesel::deserialize::{self, Queryable, QueryableByName};
use diesel::prelude::Selectable;
use diesel::row::NamedRow;
use diesel::deserialize::FromSql;

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

impl<DB> QueryableByName<DB> for Post
where
    DB: Backend,
    String: FromSql<diesel::dsl::SqlTypeOf<post::uri>, DB>,
    Option<String>: FromSql<diesel::dsl::SqlTypeOf<post::replyParent>, DB>,
    Option<i64>: FromSql<diesel::dsl::SqlTypeOf<post::sequence>, DB>
{
    fn build<'a>(row: &impl NamedRow<'a, DB>) -> deserialize::Result<Self> {
        let uri = NamedRow::get::<diesel::dsl::SqlTypeOf<post::uri>, _>(row, "uri")?;
        let cid = NamedRow::get::<diesel::dsl::SqlTypeOf<post::cid>, _>(row, "cid")?;
        let reply_parent = NamedRow::get::<diesel::dsl::SqlTypeOf<post::replyParent>, _>(row, "replyParent")?;
        let reply_root = NamedRow::get::<diesel::dsl::SqlTypeOf<post::replyRoot>, _>(row, "replyRoot")?;
        let indexed_at = NamedRow::get::<diesel::dsl::SqlTypeOf<post::indexedAt>, _>(row, "indexedAt")?;
        let prev = NamedRow::get::<diesel::dsl::SqlTypeOf<post::prev>, _>(row, "prev")?;
        let sequence = NamedRow::get::<diesel::dsl::SqlTypeOf<post::sequence>, _>(row, "sequence")?;

        Ok(Self { uri, cid, reply_parent, reply_root, indexed_at, prev, sequence })
    }
}