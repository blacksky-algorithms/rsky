use crate::xrpc_server::stream::types::{ErrorFrameBody, ErrorFrameHeader, FrameType, MessageFrameHeader};
use anyhow::Result;
use serde_json::Value;

trait Frame {
    fn get_op(&self) -> FrameType;
    
    fn to_bytes(&self) -> Result<Vec<u8>>;
    
    fn is_message(&self) -> bool;

    fn is_error(&self) -> bool;

    fn from_bytes(bytes: Vec<u8>) -> Result<FrameEnum>;
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum FrameEnum {
    MessageFrame(MessageFrame<Value>),
    ErrorFrame(ErrorFrame)
}

pub struct MessageFrameOpts {
    pub r#type: Option<String>
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MessageFrame<T> {
    pub header: MessageFrameHeader,
    pub body: T
}

impl<T> MessageFrame<T> {
    pub fn new(body: T, opts: Option<MessageFrameOpts>) -> Self {
        let header = match opts {
            None => MessageFrameHeader { op: FrameType::Message, t: None },
            Some(opts) => MessageFrameHeader { op: FrameType::Message, t: opts.r#type }
        };
        Self {
            header,
            body
        }
    }
    
    pub fn get_type(&self) -> Option<&String> {
        match self.header.t {
            None => None,
            Some(ref t) => Some(t)
        }
    }
}

impl<T> Frame for MessageFrame<T> {
    fn get_op(&self) -> FrameType {
        todo!()
    }

    fn to_bytes(&self) -> Result<Vec<u8>> {
        todo!()
    }

    fn is_message(&self) -> bool {
        todo!()
    }

    fn is_error(&self) -> bool {
        todo!()
    }

    fn from_bytes(bytes: Vec<u8>) -> Result<FrameEnum> {
        todo!()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ErrorFrame {
    pub header: ErrorFrameHeader,
    pub body: ErrorFrameBody
}

impl ErrorFrame {
    pub fn new(body: ErrorFrameBody) -> Self {
        Self {
            header: ErrorFrameHeader { op: FrameType::Error },
            body
        }
    }
    
    pub fn get_code(&self) -> &String {
        &self.body.error
    }

    pub fn get_message(&self) -> Option<&String> {
        match self.body.message {
            None => None,
            Some(ref message) => Some(message)
        }
    }
}

impl Frame for ErrorFrame {
    fn get_op(&self) -> FrameType {
        todo!()
    }

    fn to_bytes(&self) -> Result<Vec<u8>> {
        todo!()
    }

    fn is_message(&self) -> bool {
        todo!()
    }

    fn is_error(&self) -> bool {
        todo!()
    }

    fn from_bytes(bytes: Vec<u8>) -> Result<FrameEnum> {
        todo!()
    }
}
