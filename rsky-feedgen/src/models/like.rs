use diesel::prelude::*;

#[derive(Queryable, Selectable, Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::like)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Like {
    #[serde(rename = "uri")]
    pub uri: String,
    #[serde(rename = "cid")]
    pub cid: String,
    #[serde(rename = "author")]
    pub author: String,
    #[serde(rename = "subjectCid")]
    pub subject_cid: String,
    #[serde(rename = "subjectUri")]
    pub subject_uri: String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "indexedAt")]
    pub indexed_at: String,
    #[serde(rename = "prev", skip_serializing_if = "Option::is_none")]
    pub prev: Option<String>,
    #[serde(rename = "sequence")]
    pub sequence: Option<i64>,
}
