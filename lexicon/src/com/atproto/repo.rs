use cid::Cid;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct StrongRef {
    pub uri: String,
    pub cid: String,
}

#[derive(Debug, Deserialize)]
pub struct Record<T> {
    pub uri: String,
    pub cid: String,
    pub value: T,
}

#[derive(Debug, Deserialize)]
pub struct ListRecordsOutput<T> {
    pub cursor: Option<String>,
    pub records: Vec<Record<T>>,
}

#[derive(Serialize)]
pub struct CreateRecord<'a, T> {
    pub repo: &'a str,
    pub collection: &'a str,
    pub record: T,
}

#[derive(Debug, Deserialize)]
pub struct CreateRecordOutput {
    pub cid: String,
    pub uri: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateUploadBlob {
    pub blob: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Link {
    #[serde(rename(deserialize = "$link", serialize = "$link"))]
    pub link: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Blob {
    #[serde(
        rename(deserialize = "$type", serialize = "$type"),
        skip_serializing_if = "Option::is_none"
    )]
    pub rust_type: Option<String>,
    pub r#ref: Cid,
    #[serde(rename(deserialize = "mimeType", serialize = "mimeType"))]
    pub mime_type: String,
    pub size: usize,
    pub original: OriginalBlob,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OriginalBlob {
    #[serde(
        rename(deserialize = "$type", serialize = "$type"),
        skip_serializing_if = "Option::is_none"
    )]
    pub rust_type: Option<String>,
    pub r#ref: Cid,
    #[serde(rename(deserialize = "mimeType", serialize = "mimeType"))]
    pub mime_type: String,
    pub size: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BlobOutput {
    pub blob: Blob,
}
