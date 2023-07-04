use diesel::prelude::*;

#[derive(Queryable, Selectable, Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::sub_state)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct SubState {
    #[serde(rename = "service")]
    pub service: String,
    #[serde(rename = "cursor")]
    pub cursor: i64,
}
