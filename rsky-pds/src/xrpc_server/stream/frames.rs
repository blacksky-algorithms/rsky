use crate::common::struct_to_cbor;
use crate::xrpc_server::stream::types::{
    ErrorFrameBody, ErrorFrameHeader, FrameType, MessageFrameHeader,
};
use anyhow::Result;
use serde_json::Value;

pub trait Frame {
    fn get_op(&self) -> &FrameType;

    fn to_bytes(&self) -> Result<Vec<u8>>;

    fn is_message(&self) -> bool;

    fn is_error(&self) -> bool;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FrameEnum {
    ErrorFrame(ErrorFrame), // Intentionally try to decode as Error first
    MessageFrame(MessageFrame<Value>),
}

pub struct MessageFrameOpts {
    pub r#type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessageFrame<T> {
    pub header: MessageFrameHeader,
    pub body: T,
}

impl<T> MessageFrame<T> {
    pub fn new(body: T, opts: Option<MessageFrameOpts>) -> Self {
        let header = match opts {
            None => MessageFrameHeader {
                op: FrameType::Message,
                t: None,
            },
            Some(opts) => MessageFrameHeader {
                op: FrameType::Message,
                t: opts.r#type,
            },
        };
        Self { header, body }
    }

    pub fn get_type(&self) -> Option<&String> {
        match self.header.t {
            None => None,
            Some(ref t) => Some(t),
        }
    }
}

impl<T: serde::Serialize> Frame for MessageFrame<T> {
    fn get_op(&self) -> &FrameType {
        &self.header.op
    }

    fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok([struct_to_cbor(&self.header)?, struct_to_cbor(&self.body)?].concat())
    }

    fn is_message(&self) -> bool {
        *self.get_op() == FrameType::Message
    }

    fn is_error(&self) -> bool {
        *self.get_op() == FrameType::Error
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorFrame {
    pub header: ErrorFrameHeader,
    pub body: ErrorFrameBody,
}

impl ErrorFrame {
    pub fn new(body: ErrorFrameBody) -> Self {
        Self {
            header: ErrorFrameHeader {
                op: FrameType::Error,
            },
            body,
        }
    }

    pub fn get_code(&self) -> &String {
        &self.body.error
    }

    pub fn get_message(&self) -> Option<&String> {
        match self.body.message {
            None => None,
            Some(ref message) => Some(message),
        }
    }
}

impl Frame for ErrorFrame {
    fn get_op(&self) -> &FrameType {
        &self.header.op
    }

    fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok([
            serde_ipld_dagcbor::to_vec(&self.header)?,
            serde_ipld_dagcbor::to_vec(&self.body)?,
        ]
        .concat())
    }

    fn is_message(&self) -> bool {
        *self.get_op() == FrameType::Message
    }

    fn is_error(&self) -> bool {
        *self.get_op() == FrameType::Error
    }
}
