use serde_repr::{Deserialize_repr, Serialize_repr};

#[derive(Debug, Clone, PartialEq, Serialize_repr, Deserialize_repr)]
#[repr(i8)]
pub enum FrameType {
    Message = 1,
    Error = -1,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessageFrameHeader {
    pub op: FrameType,     // Frame op
    pub t: Option<String>, // Message body type discriminator
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorFrameHeader {
    pub op: FrameType, // Frame op
                       // `t` Should not be included in header if op is -1.
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorFrameBody {
    pub error: String,           // Error code
    pub message: Option<String>, // Error message
}

/// Body of a `#info` message frame on a subscription stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InfoFrameBody {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FrameHeader {
    MessageFrameHeader(MessageFrameHeader),
    ErrorFrameHeader(ErrorFrameHeader),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CloseCode {
    Normal = 1000,
    Abnormal = 1006,
    Policy = 1008,
}
