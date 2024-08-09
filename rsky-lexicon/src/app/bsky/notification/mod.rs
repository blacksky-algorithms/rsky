#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterPushInput {
    pub service_did: String,
    pub token: String,
    pub platform: String,
    pub app_id: String,
}
