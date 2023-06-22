use std::error::Error;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct ValidationErrorMessageResponse {
    #[serde(rename = "code", skip_serializing_if = "Option::is_none")]
    pub code: Option<crate::models::ErrorCode>,
    #[serde(rename = "message", skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl ValidationErrorMessageResponse {
    pub fn new() -> ValidationErrorMessageResponse {
        ValidationErrorMessageResponse {
            code: None,
            message: None,
        }
    }
}

impl fmt::Display for ValidationErrorMessageResponse {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut message = "".to_owned();
        if let Some(error_message) = &self.message {
            message = error_message.clone();
        }
        write!(f, "validation_error: {}", message)
    }
}

impl Error for ValidationErrorMessageResponse {}
