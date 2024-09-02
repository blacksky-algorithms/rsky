

#[repr(i8)]
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum FrameType {
    Message = 1,
    Error = -1,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MessageFrameHeader {
    pub op: FrameType, // Frame op
    pub t: Option<String> // Message body type discriminator
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ErrorFrameHeader {
    pub op: FrameType, // Frame op
    // `t` Should not be included in header if op is -1.
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ErrorFrameBody {
    pub error: String, // Error code
    pub message: Option<String> // Error message
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum FrameHeader {
    MessageFrameHeader(MessageFrameHeader),
    ErrorFrameHeader(ErrorFrameHeader)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum CloseCode {
    Normal = 1000,
    Abnormal = 1006,
    Policy = 1008
}