#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct InternalErrorMessageResponse {
    #[serde(rename = "code", skip_serializing_if = "Option::is_none")]
    pub code: Option<crate::models::InternalErrorCode>,
    #[serde(rename = "message", skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl InternalErrorMessageResponse {
    pub fn new() -> InternalErrorMessageResponse {
        InternalErrorMessageResponse {
            code: None,
            message: None,
        }
    }
}
