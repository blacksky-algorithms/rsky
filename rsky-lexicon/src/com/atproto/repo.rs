use crate::com::atproto::sync::{default_resource, deserialize_option_cid_v1};
use lexicon_cid::Cid;
use serde::ser::{SerializeMap, Serializer};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct StrongRef {
    pub uri: String,
    pub cid: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Record {
    pub uri: String,
    pub cid: String,
    pub value: Value,
}

fn serialize_option_cid_as_link<S>(
    cid_option: &Option<Cid>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match cid_option {
        Some(cid) => {
            let mut map = serializer.serialize_map(Some(1))?;
            map.serialize_entry("$link", &cid.to_string())?;
            map.end()
        }
        None => serializer.serialize_none(),
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Blob {
    #[serde(
        rename(deserialize = "$type", serialize = "$type"),
        skip_serializing_if = "Option::is_none"
    )]
    pub r#type: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default = "default_resource",
        deserialize_with = "deserialize_option_cid_v1",
        serialize_with = "serialize_option_cid_as_link"
    )]
    pub r#ref: Option<Cid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,
    #[serde(rename(deserialize = "mimeType", serialize = "mimeType"))]
    pub mime_type: String,
    pub size: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original: Option<OriginalBlob>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct OriginalBlob {
    #[serde(
        rename(deserialize = "$type", serialize = "$type"),
        skip_serializing_if = "Option::is_none"
    )]
    pub r#type: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default = "default_resource",
        deserialize_with = "deserialize_option_cid_v1"
    )]
    pub r#ref: Option<Cid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,
    #[serde(rename(deserialize = "mimeType", serialize = "mimeType"))]
    pub mime_type: String,
    pub size: i64,
}

/// Create a single new repository record. Requires auth, implemented by PDS.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
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
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
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
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
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

/// Apply a batch transaction of repository creates, updates, and deletes.
/// Requires auth, implemented by PDS.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ApplyWritesInput {
    /// The handle or DID of the repo (aka, current account).
    pub repo: String,
    /// Can be set to 'false' to skip Lexicon schema validation of record data, for all operations.
    pub validate: Option<bool>,
    /// The Record Key.
    pub writes: Vec<ApplyWritesInputRefWrite>,
    /// Compare and swap with the previous commit by CID.
    #[serde(rename = "swapCommit", skip_serializing_if = "Option::is_none")]
    pub swap_commit: Option<String>,
}

/// Get a single record from a repository. Does not require auth.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct GetRecordOutput {
    pub uri: String,
    /// The CID of the version of the record. If not specified, then return the most recent version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ListRecordsOutput {
    pub cursor: Option<String>,
    pub records: Vec<Record>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct CreateRecordOutput {
    pub cid: String,
    pub uri: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct PutRecordOutput {
    pub cid: String,
    pub uri: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct BlobOutput {
    pub blob: Blob,
}

/// Returns a list of missing blobs for the requesting account.
/// Intended to be used in the account migration flow.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ListMissingBlobsOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub blobs: Vec<ListMissingBlobsRefRecordBlob>,
}

/// Get information about an account and repository, including the list of collections.
/// Does not require auth.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
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

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
pub enum ApplyWritesInputRefWrite {
    #[serde(rename = "com.atproto.repo.applyWrites#create")]
    Create(RefWriteCreate),
    #[serde(rename = "com.atproto.repo.applyWrites#update")]
    Update(RefWriteUpdate),
    #[serde(rename = "com.atproto.repo.applyWrites#delete")]
    Delete(RefWriteDelete),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ListMissingBlobsRefRecordBlob {
    pub cid: String,
    pub record_uri: String,
}

/// Operation which creates a new record.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RefWriteCreate {
    pub collection: String,
    pub rkey: Option<String>,
    pub value: Value,
}

/// Operation which updates an existing record.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RefWriteUpdate {
    pub collection: String,
    pub rkey: String,
    pub value: Value,
}

/// Operation which deletes an existing record.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RefWriteDelete {
    pub collection: String,
    pub rkey: String,
}
