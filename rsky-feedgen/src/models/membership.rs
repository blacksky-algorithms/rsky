use diesel::prelude::*;

#[derive(Queryable, Selectable, Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::membership)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Membership {
    #[serde(rename = "did")]
    pub did: String,
    #[serde(rename = "included")]
    pub included: bool,
    #[serde(rename = "excluded")]
    pub excluded: bool,
    #[serde(rename = "list")]
    pub list: String,
}
