use crate::schema::post;
use diesel::backend::Backend;
use diesel::deserialize::FromSql;
use diesel::deserialize::{self, Queryable, QueryableByName};
use diesel::prelude::Selectable;
use diesel::row::NamedRow;

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
    #[serde(rename = "text", skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(rename = "lang", skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(rename = "author")]
    pub author: String,
    #[serde(rename = "externalUri", skip_serializing_if = "Option::is_none")]
    pub external_uri: Option<String>,
    #[serde(rename = "externalTitle", skip_serializing_if = "Option::is_none")]
    pub external_title: Option<String>,
    #[serde(
        rename = "externalDescription",
        skip_serializing_if = "Option::is_none"
    )]
    pub external_description: Option<String>,
    #[serde(rename = "externalThumb", skip_serializing_if = "Option::is_none")]
    pub external_thumb: Option<String>,
    #[serde(rename = "quoteCid", skip_serializing_if = "Option::is_none")]
    pub quote_cid: Option<String>,
    #[serde(rename = "quoteUri", skip_serializing_if = "Option::is_none")]
    pub quote_uri: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
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
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
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
            text: row.7,
            lang: row.8,
            author: row.9,
            external_uri: row.10,
            external_title: row.11,
            external_description: row.12,
            external_thumb: row.13,
            quote_cid: row.14,
            quote_uri: row.15,
            created_at: row.16,
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
        post::text,
        post::lang,
        post::author,
        post::externalUri,
        post::externalTitle,
        post::externalDescription,
        post::externalThumb,
        post::quoteCid,
        post::quoteUri,
        post::createdAt,
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
            post::text,
            post::lang,
            post::author,
            post::externalUri,
            post::externalTitle,
            post::externalDescription,
            post::externalThumb,
            post::quoteCid,
            post::quoteUri,
            post::createdAt,
        )
    }
}

impl<DB> QueryableByName<DB> for Post
where
    DB: Backend,
    String: FromSql<diesel::dsl::SqlTypeOf<post::uri>, DB>,
    Option<String>: FromSql<diesel::dsl::SqlTypeOf<post::replyParent>, DB>,
    Option<i64>: FromSql<diesel::dsl::SqlTypeOf<post::sequence>, DB>,
{
    fn build<'a>(row: &impl NamedRow<'a, DB>) -> deserialize::Result<Self> {
        let uri = NamedRow::get::<diesel::dsl::SqlTypeOf<post::uri>, _>(row, "uri")?;
        let cid = NamedRow::get::<diesel::dsl::SqlTypeOf<post::cid>, _>(row, "cid")?;
        let reply_parent =
            NamedRow::get::<diesel::dsl::SqlTypeOf<post::replyParent>, _>(row, "replyParent")?;
        let reply_root =
            NamedRow::get::<diesel::dsl::SqlTypeOf<post::replyRoot>, _>(row, "replyRoot")?;
        let indexed_at =
            NamedRow::get::<diesel::dsl::SqlTypeOf<post::indexedAt>, _>(row, "indexedAt")?;
        let prev = NamedRow::get::<diesel::dsl::SqlTypeOf<post::prev>, _>(row, "prev")?;
        let sequence = NamedRow::get::<diesel::dsl::SqlTypeOf<post::sequence>, _>(row, "sequence")?;
        let text = NamedRow::get::<diesel::dsl::SqlTypeOf<post::text>, _>(row, "text")?;
        let lang = NamedRow::get::<diesel::dsl::SqlTypeOf<post::lang>, _>(row, "lang")?;

        let author = NamedRow::get::<diesel::dsl::SqlTypeOf<post::author>, _>(row, "author")?;
        let external_uri =
            NamedRow::get::<diesel::dsl::SqlTypeOf<post::externalUri>, _>(row, "externalUri")?;
        let external_title =
            NamedRow::get::<diesel::dsl::SqlTypeOf<post::externalTitle>, _>(row, "externalTitle")?;
        let external_description = NamedRow::get::<
            diesel::dsl::SqlTypeOf<post::externalDescription>,
            _,
        >(row, "externalDescription")?;
        let external_thumb =
            NamedRow::get::<diesel::dsl::SqlTypeOf<post::externalThumb>, _>(row, "externalThumb")?;
        let quote_cid =
            NamedRow::get::<diesel::dsl::SqlTypeOf<post::quoteCid>, _>(row, "quoteCid")?;
        let quote_uri =
            NamedRow::get::<diesel::dsl::SqlTypeOf<post::quoteUri>, _>(row, "quoteUri")?;
        let created_at =
            NamedRow::get::<diesel::dsl::SqlTypeOf<post::createdAt>, _>(row, "createdAt")?;
        Ok(Self {
            uri,
            cid,
            reply_parent,
            reply_root,
            indexed_at,
            prev,
            sequence,
            text,
            lang,
            author,
            external_uri,
            external_title,
            external_description,
            external_thumb,
            quote_cid,
            quote_uri,
            created_at,
        })
    }
}
