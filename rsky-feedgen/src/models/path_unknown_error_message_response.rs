#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct PathUnknownErrorMessageResponse {
    #[serde(rename = "code", skip_serializing_if = "Option::is_none")]
    pub code: Option<crate::models::NotFoundErrorCode>,
    #[serde(rename = "message", skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl PathUnknownErrorMessageResponse {
    pub fn new() -> PathUnknownErrorMessageResponse {
        PathUnknownErrorMessageResponse {
            code: None,
            message: None,
        }
    }
}
