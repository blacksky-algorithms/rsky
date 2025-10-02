use diesel::prelude::*;

#[derive(Queryable, Selectable, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::banned_from_tv)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BannedFromTv {
    #[serde(rename = "did")]
    pub did: String,
    #[serde(rename = "reason", skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(rename = "createdAt", skip_serializing_if = "Option::is_none")]
    #[diesel(column_name = createdAt)]
    pub created_at: Option<String>,
    #[serde(rename = "tags", skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<Option<String>>>,
}
