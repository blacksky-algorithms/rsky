use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
pub struct StrongRef {
    pub uri: String,
    pub cid: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub uri: String,
    pub cid: String,
    pub value: Value,
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
    pub r#type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<Link>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,
    #[serde(rename(deserialize = "mimeType", serialize = "mimeType"))]
    pub mime_type: String,
    pub size: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original: Option<OriginalBlob>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OriginalBlob {
    #[serde(
    rename(deserialize = "$type", serialize = "$type"),
    skip_serializing_if = "Option::is_none"
    )]
    pub r#type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<Link>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,
    #[serde(rename(deserialize = "mimeType", serialize = "mimeType"))]
    pub mime_type: String,
    pub size: i64,
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

/// Write a repository record, creating or updating it as needed. Requires auth, implemented by PDS.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PutRecordInput {
    /// The handle or DID of the repo (aka, current account)
    pub repo: String,
    /// The NSID of the record collection.
    pub collection: String,
    /// The Record Key.
    pub rkey: String,
    /// Can be set to 'false' to skip Lexicon schema validation of record data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validate: Option<bool>, // Default 'true'
    /// The record itself. Must contain a $type field.
    pub record: Value,
    /// Compare and swap with the previous commit by CID.
    #[serde(rename = "swapRecord")]
    pub swap_record: Option<String>,
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

#[derive(Debug, Serialize, Deserialize)]
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
pub struct PutRecordOutput {
    pub cid: String,
    pub uri: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BlobOutput {
    pub blob: Blob,
}

/// Get information about an account and repository, including the list of collections. 
/// Does not require auth.
#[derive(Debug, Serialize, Deserialize)]
pub struct DescribeRepoOutput {
    pub handle: String,
    pub did: String,
    /// The complete DID document for this account.
    #[serde(rename = "didDoc")]
    pub did_doc: Value,
    /// List of all the collections (NSIDs) for which this repo contains at least one record.
    pub collections: Vec<String>,
    /// Indicates if handle is currently valid (resolves bi-directionally)
    #[serde(rename = "handleIsCorrect")]
    pub handle_is_correct: bool,
}