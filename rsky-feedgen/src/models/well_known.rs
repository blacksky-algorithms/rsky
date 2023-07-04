#[derive(Debug, Serialize, Deserialize)]
pub struct WellKnown {
    #[serde(rename = "@context")]
    pub context: Vec<String>,
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "service")]
    pub service: Vec<crate::models::KnownService>,
}
