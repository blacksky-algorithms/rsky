use std::error::Error;
use std::fmt;

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

impl fmt::Display for PathUnknownErrorMessageResponse {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut message = "".to_owned();
        if let Some(error_message) = &self.message {
            message = error_message.clone();
        }
        write!(f, "not_found_error: {}", message)
    }
}

impl Error for PathUnknownErrorMessageResponse {}
