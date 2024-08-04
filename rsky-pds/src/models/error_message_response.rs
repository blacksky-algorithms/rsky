#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct ErrorMessageResponse {
    #[serde(rename = "code", skip_serializing_if = "Option::is_none")]
    pub code: Option<crate::models::ErrorCode>,
    #[serde(rename = "message", skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl ErrorMessageResponse {
    pub fn new() -> ErrorMessageResponse {
        ErrorMessageResponse {
            code: None,
            message: None,
        }
    }
}
