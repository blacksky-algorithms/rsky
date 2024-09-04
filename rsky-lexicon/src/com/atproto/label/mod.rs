use chrono::{DateTime, Utc};

/// Metadata tag on an atproto resource (eg, repo or record).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Label {
    /// The AT Protocol version of the label object.
    pub ver: Option<u8>,
    /// DID of the actor who created this label.
    pub src: String,
    /// AT URI of the record, repository (account), or other resource that this label applies to.
    pub uri: String,
    /// Optionally, CID specifying the specific version of 'uri' resource this label applies to.
    pub cid: Option<String>,
    /// The short string name of the value or type of this label.
    pub val: String,
    /// If true, this is a negation label, overwriting a previous label.
    pub neg: Option<bool>,
    /// Timestamp when this label was created.
    pub cts: DateTime<Utc>,
    /// Timestamp at which this label expires (no longer applies).
    pub exp: Option<DateTime<Utc>>,
    /// Signature of dag-cbor encoded label.
    pub sig: Option<Vec<u8>>,
}

/// Metadata tags on an atproto record, published by the author within the record
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct SelfLabels {
    pub values: Vec<SelfLabel>,
}

/// Metadata tag on an atproto record, published by the author within the record.
/// Note that schemas should use #selfLabels, not #selfLabel.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct SelfLabel {
    /// The short string name of the value or type of this label.
    pub val: String,
}
