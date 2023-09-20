use cid::Cid;
use serde::{Deserializer, Deserialize, Serialize};
use serde_cbor::tags::Tagged;

const CBOR_TAG_CID: u64 = 42;
const MULTIBASE_IDENTITY: u8 = 0;


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
    #[serde(deserialize_with = "deserialize_cid_v1")]
    pub r#ref: Cid,
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
    #[serde(deserialize_with = "deserialize_cid_v1")]
    pub r#ref: Cid,
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

fn deserialize_cid_v1<'de, D>(deserializer: D) -> Result<cid::Cid, D::Error>
where D: Deserializer<'de> {
    let buf = Tagged::<serde_bytes::ByteBuf>::deserialize(deserializer)?;
    match buf.tag {
        Some(CBOR_TAG_CID) | None => {
            let mut bz = buf.value.into_vec();

            if bz.first() == Some(&MULTIBASE_IDENTITY) {
                bz.remove(0);
            }

            Ok(Cid::try_from(bz)
                .map_err(|e| serde::de::Error::custom(format!("Failed to deserialize Cid: {}", e)))?)
        }
        Some(_) => Err(serde::de::Error::custom("unexpected tag")),
    }
}
