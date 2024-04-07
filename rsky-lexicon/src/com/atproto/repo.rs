use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
pub struct StrongRef {
    pub uri: String,
    pub cid: String,
}

#[derive(Debug, Deserialize)]
pub struct Record {
    pub uri: String,
    pub cid: String,
    pub value: Value,
}

/// Create a single new repository record. Requires auth, implemented by PDS.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateRecordInput {
    /// The handle or DID of the repo (aka, current account)
    pub repo: String,
    /// The NSID of the record collection.
    pub collection: String,
    /// The record itself. Must contain a $type field.
    pub record: Value,
    /// The Record Key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rkey: Option<String>,
    /// Can be set to 'false' to skip Lexicon schema validation of record data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validate: Option<bool>,
    /// Compare and swap with the previous commit by CID.
    #[serde(rename = "swapCommit", skip_serializing_if = "Option::is_none")]
    pub swap_commit: Option<String>,
}

/// Delete a repository record, or ensure it doesn't exist. Requires auth, implemented by PDS.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeleteRecordInput {
    /// The handle or DID of the repo (aka, current account).
    pub repo: String,
    /// The NSID of the record collection.
    pub collection: String,
    /// The Record Key.
    pub rkey: String,
    /// Compare and swap with the previous record by CID.
    #[serde(rename = "swapRecord", skip_serializing_if = "Option::is_none")]
    pub swap_record: Option<String>,
    /// Compare and swap with the previous commit by CID.
    #[serde(rename = "swapCommit", skip_serializing_if = "Option::is_none")]
    pub swap_commit: Option<String>,
}

/// Get a single record from a repository. Does not require auth.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetRecordOutput {
    pub uri: String,
    /// The CID of the version of the record. If not specified, then return the most recent version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,
    pub value: Value,
}

#[derive(Debug, Deserialize)]
pub struct ListRecordsOutput {
    pub cursor: Option<String>,
    pub records: Vec<Record>,
}

#[derive(Debug, Serialize, Deserialize)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,
    #[serde(rename(deserialize = "mimeType", serialize = "mimeType"))]
    pub mime_type: String,
    pub size: Option<usize>,
    pub original: Option<OriginalBlob>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OriginalBlob {
    #[serde(
        rename(deserialize = "$type", serialize = "$type"),
        skip_serializing_if = "Option::is_none"
    )]
    pub rust_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,
    #[serde(rename(deserialize = "mimeType", serialize = "mimeType"))]
    pub mime_type: String,
    pub size: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BlobOutput {
    pub blob: Blob,
}
