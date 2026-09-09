use chrono::{DateTime, Utc};
use lexicon_cid::Cid;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_cbor::tags::Tagged;
use serde_json::Value;
use std::fmt;

const CBOR_TAG_CID: u64 = 42;
const MULTIBASE_IDENTITY: u8 = 0;

#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposCommitOperation {
    pub path: String,
    pub action: String,
    pub cid: Option<Cid>,
    /// For updates and deletes, the previous record CID. For creates, omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev: Option<Cid>,
}

/// Represents an update of repository state. Note that empty commits are allowed,
/// which include no repo data changes, but an update to rev and signature.
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposCommit {
    pub seq: i64,
    #[serde(serialize_with = "serialize_datetime_ms")]
    pub time: DateTime<Utc>,
    pub rebase: bool,
    #[serde(rename = "tooBig")]
    pub too_big: bool,
    pub repo: String,
    #[serde(deserialize_with = "deserialize_cid_v1")]
    pub commit: Cid,
    /// DEPRECATED -- unused in sync v1.1. Retained for deserializing legacy events.
    #[serde(
        default = "default_resource",
        deserialize_with = "deserialize_option_cid_v1",
        skip_serializing_if = "Option::is_none"
    )]
    pub prev: Option<Cid>,
    pub rev: String,
    pub since: Option<String>,
    #[serde(with = "serde_bytes")]
    pub blocks: Vec<u8>,
    pub ops: Vec<SubscribeReposCommitOperation>,
    /// CIDs of blobs referenced by records in this commit. The lexicon defines this as an
    /// array of `cid-link`, so it serializes as DAG-CBOR tag-42 links. Decoding is lenient
    /// and also accepts text-string CIDs, which older rsky-pds instances emitted.
    #[serde(default, deserialize_with = "deserialize_cid_vec_v1")]
    pub blobs: Vec<Cid>,
    /// The root CID of the MST tree for the previous commit from this repo.
    /// Effectively required for the inductive version of the firehose.
    #[serde(rename = "prevData", default, skip_serializing_if = "Option::is_none")]
    pub prev_data: Option<Cid>,
}

/// Get the current commit CID & revision of the specified repo. Does not require auth.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetLatestCommitOutput {
    pub cid: String,
    pub rev: String,
}

/// DEPRECATED - please use com.atproto.sync.getLatestCommit instead
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetHeadOutput {
    pub root: String,
}

/// Get the hosting status for a repository, on this server.
/// Expected to be implemented by PDS and Relay.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetRepoStatusOutput {
    pub did: String,
    pub active: bool,
    // If active=false, this optional field indicates a possible reason for why the account
    // is not active. If active=false and no status is supplied, then the host makes no claim for
    // why the repository is no longer being hosted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<RepoStatus>,
    // Optional field, the current rev of the repo, if active=true
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
}

/// List blob CIDs for an account, since some repo revision. Does not require auth;
/// implemented by PDS
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListBlobsOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub cids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListReposOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub repos: Vec<RefRepo>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepoStatus {
    Takedown,
    Suspended,
    Deactivated,
    Deleted,
    Desynchronized,
    Throttled,
}

/// DEPRECATED -- Use #identity event instead
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposHandle {
    pub did: String,
    pub handle: String,
    pub seq: i64,
    #[serde(serialize_with = "serialize_datetime_ms")]
    pub time: DateTime<Utc>,
}

/// Represents a change to an account's identity. Could be an updated handle, signing key, or
/// pds hosting endpoint. Serves as a prod to all downstream services to refresh their identity cache.
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposIdentity {
    pub did: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    pub seq: i64,
    #[serde(serialize_with = "serialize_datetime_ms")]
    pub time: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposSync {
    pub seq: i64,
    pub did: String,
    #[serde(with = "serde_bytes")]
    pub blocks: Vec<u8>,
    pub rev: String,
    #[serde(serialize_with = "serialize_datetime_ms")]
    pub time: DateTime<Utc>,
}

/// Represents a change to an account's identity. Could be an updated handle, signing key, or
/// pds hosting endpoint. Serves as a prod to all downstream services to refresh their identity cache.
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposAccount {
    pub seq: i64,
    pub did: String,
    #[serde(serialize_with = "serialize_datetime_ms")]
    pub time: DateTime<Utc>,
    pub active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<AccountStatus>,
}

/// If active=false, this optional field indicates a reason for why the account is not active.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountStatus {
    Takendown,
    Suspended,
    Deleted,
    Deactivated,
    Desynchronized,
    Throttled,
}

impl fmt::Display for AccountStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// DEPRECATED -- Use #account event instead
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposTombstone {
    pub did: String,
    pub seq: i64,
    #[serde(serialize_with = "serialize_datetime_ms")]
    pub time: DateTime<Utc>,
}

#[derive(Debug)]
pub enum SubscribeRepos {
    Commit(SubscribeReposCommit),
    Identity(SubscribeReposIdentity),
    Account(SubscribeReposAccount),
    Handle(SubscribeReposHandle),
    Tombstone(SubscribeReposTombstone),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RefRepo {
    pub did: String,
    // Current repo commit CID
    pub head: String,
    pub rev: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    // If active=false, this optional field indicates a possible reason for why the account
    // is not active. If active=false and no status is supplied, then the host makes no claim for
    // why the repository is no longer being hosted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<RepoStatus>,
}

pub fn deserialize_cid_v1<'de, D>(deserializer: D) -> Result<Cid, D::Error>
where
    D: Deserializer<'de>,
{
    let buf = Tagged::<serde_bytes::ByteBuf>::deserialize(deserializer)?;
    match buf.tag {
        Some(CBOR_TAG_CID) | None => {
            let mut bz = buf.value.into_vec();

            if bz.first() == Some(&MULTIBASE_IDENTITY) {
                bz.remove(0);
            }

            Ok(Cid::try_from(bz).map_err(|e| {
                serde::de::Error::custom(format!("Failed to deserialize Cid: {}", e))
            })?)
        }
        Some(_) => Err(serde::de::Error::custom("unexpected tag")),
    }
}

pub fn deserialize_option_cid_v1<'de, D>(deserializer: D) -> Result<Option<Cid>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum BlobFormat {
        Legacy(Tagged<serde_bytes::ByteBuf>),
        Link(serde_json::Map<String, Value>),
    }

    // Deserialize into an optional map, expecting an object like {"$link": "cid_string"}
    let opt_blob = Option::<BlobFormat>::deserialize(deserializer)?;

    match opt_blob {
        // If there's no object, return None
        None => Ok(None),
        Some(BlobFormat::Link(map)) => {
            // Check if the map contains the "$link" key
            if let Some(Value::String(link)) = map.get("$link") {
                // Attempt to parse the CID from the string value
                Cid::try_from(link.as_str())
                    .map(Some)
                    .map_err(serde::de::Error::custom)
            } else {
                // Return error if "$link" is missing or not a string
                Err(serde::de::Error::custom(
                    "expected \"$link\" field with CID string",
                ))
            }
        }
        Some(BlobFormat::Legacy(buf)) => match buf.tag {
            Some(CBOR_TAG_CID) | None => {
                let mut bz = buf.value.into_vec();

                if bz.first() == Some(&MULTIBASE_IDENTITY) {
                    bz.remove(0);
                }

                Ok(Some(Cid::try_from(bz).map_err(|e| {
                    serde::de::Error::custom(format!("Failed to deserialize Cid: {}", e))
                })?))
            }
            Some(_) => Err(serde::de::Error::custom("unexpected tag")),
        },
    }
}

/// Deserializes an array of CIDs (e.g. `#commit.blobs`) leniently.
///
/// The lexicon declares the elements as `cid-link`, which DAG-CBOR encodes as tag 42
/// wrapping the binary CID; that is what the reference PDS and `tranquil` emit and what
/// this struct serializes. Older rsky-pds instances emitted text-string CIDs instead, and
/// those frames still exist in the wild, so string elements (and raw untagged CID bytes)
/// are accepted too. Like `deserialize_cid_v1`, this drives the deserializer through
/// `deserialize_any` so a tag-42 link arrives via `visit_newtype_struct`.
pub fn deserialize_cid_vec_v1<'de, D>(deserializer: D) -> Result<Vec<Cid>, D::Error>
where
    D: Deserializer<'de>,
{
    struct LenientCid(Cid);

    fn cid_from_bytes<E: serde::de::Error>(mut bz: Vec<u8>) -> Result<LenientCid, E> {
        if bz.first() == Some(&MULTIBASE_IDENTITY) {
            bz.remove(0);
        }
        Cid::try_from(bz)
            .map(LenientCid)
            .map_err(|e| E::custom(format!("Failed to deserialize Cid: {}", e)))
    }

    struct LenientCidVisitor;

    impl<'de> serde::de::Visitor<'de> for LenientCidVisitor {
        type Value = LenientCid;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a CID link (CBOR tag 42) or a CID string")
        }

        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<LenientCid, E> {
            Cid::try_from(v)
                .map(LenientCid)
                .map_err(|e| E::custom(format!("Failed to deserialize Cid: {}", e)))
        }

        fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<LenientCid, E> {
            cid_from_bytes(v.to_vec())
        }

        fn visit_byte_buf<E: serde::de::Error>(self, v: Vec<u8>) -> Result<LenientCid, E> {
            cid_from_bytes(v)
        }

        // serde_ipld_dagcbor (and serde_cbor) surface a tagged value as a newtype struct
        // wrapping the tag's content; for tag 42 that content is the binary CID.
        fn visit_newtype_struct<D2>(self, deserializer: D2) -> Result<LenientCid, D2::Error>
        where
            D2: Deserializer<'de>,
        {
            let buf = serde_bytes::ByteBuf::deserialize(deserializer)?;
            cid_from_bytes(buf.into_vec())
        }

        // JSON form: {"$link": "cid_string"}
        fn visit_map<A>(self, map: A) -> Result<LenientCid, A::Error>
        where
            A: serde::de::MapAccess<'de>,
        {
            let map = serde_json::Map::<String, Value>::deserialize(
                serde::de::value::MapAccessDeserializer::new(map),
            )?;
            match map.get("$link") {
                Some(Value::String(link)) => Cid::try_from(link.as_str())
                    .map(LenientCid)
                    .map_err(serde::de::Error::custom),
                _ => Err(serde::de::Error::custom(
                    "expected \"$link\" field with CID string",
                )),
            }
        }
    }

    impl<'de> Deserialize<'de> for LenientCid {
        fn deserialize<D2>(deserializer: D2) -> Result<Self, D2::Error>
        where
            D2: Deserializer<'de>,
        {
            deserializer.deserialize_any(LenientCidVisitor)
        }
    }

    let cids = Vec::<LenientCid>::deserialize(deserializer)?;
    Ok(cids.into_iter().map(|c| c.0).collect())
}

pub fn default_resource() -> Option<Cid> {
    None
}

/// Serializes event timestamps with millisecond precision and a `Z` suffix,
/// matching the reference implementation's `Date.toISOString()` output.
pub fn serialize_datetime_ms<S>(dt: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&format!("{}", dt.format("%Y-%m-%dT%H:%M:%S%.3fZ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    const TEST_CID: &str = "bafkreibjfgx2gprinfvicegelk5kosd6y2frmqpqzwqkg7usac74l3t2v4";

    fn test_time() -> DateTime<Utc> {
        "2026-01-12T19:45:23.307Z".parse::<DateTime<Utc>>().unwrap()
    }

    fn test_commit() -> SubscribeReposCommit {
        SubscribeReposCommit {
            seq: 1,
            time: test_time(),
            rebase: false,
            too_big: false,
            repo: "did:plc:test".to_string(),
            commit: Cid::from_str(TEST_CID).unwrap(),
            prev: None,
            rev: "3jzfcijpj2z2a".to_string(),
            since: None,
            blocks: vec![1, 2, 3],
            ops: vec![SubscribeReposCommitOperation {
                path: "app.bsky.feed.post/3jzfcijpj2z2a".to_string(),
                action: "create".to_string(),
                cid: Some(Cid::from_str(TEST_CID).unwrap()),
                prev: None,
            }],
            blobs: vec![],
            prev_data: None,
        }
    }

    #[test]
    fn commit_omits_deprecated_prev_and_absent_prev_data() {
        let value = serde_json::to_value(test_commit()).unwrap();
        let obj = value.as_object().unwrap();
        assert!(!obj.contains_key("prev"));
        assert!(!obj.contains_key("prevData"));
        assert_eq!(obj["time"], "2026-01-12T19:45:23.307Z");
        let op = value["ops"][0].as_object().unwrap();
        assert!(!op.contains_key("prev"));
        assert!(op.contains_key("cid"));
    }

    #[test]
    fn commit_includes_prev_data_and_op_prev_when_present() {
        let mut commit = test_commit();
        commit.prev_data = Some(Cid::from_str(TEST_CID).unwrap());
        commit.ops[0].prev = Some(Cid::from_str(TEST_CID).unwrap());
        commit.ops[0].action = "update".to_string();
        let value = serde_json::to_value(commit).unwrap();
        assert!(value.as_object().unwrap().contains_key("prevData"));
        assert!(value["ops"][0].as_object().unwrap().contains_key("prev"));
    }

    #[test]
    fn identity_omits_absent_handle() {
        let identity = SubscribeReposIdentity {
            did: "did:plc:test".to_string(),
            handle: None,
            seq: 1,
            time: test_time(),
        };
        let value = serde_json::to_value(identity).unwrap();
        assert!(!value.as_object().unwrap().contains_key("handle"));

        let identity = SubscribeReposIdentity {
            did: "did:plc:test".to_string(),
            handle: Some("alice.test".to_string()),
            seq: 1,
            time: test_time(),
        };
        let value = serde_json::to_value(identity).unwrap();
        assert_eq!(value["handle"], "alice.test");
    }

    #[test]
    fn account_omits_absent_status() {
        let account = SubscribeReposAccount {
            seq: 1,
            did: "did:plc:test".to_string(),
            time: test_time(),
            active: true,
            status: None,
        };
        let value = serde_json::to_value(account).unwrap();
        assert!(!value.as_object().unwrap().contains_key("status"));

        let account = SubscribeReposAccount {
            seq: 1,
            did: "did:plc:test".to_string(),
            time: test_time(),
            active: false,
            status: Some(AccountStatus::Takendown),
        };
        let value = serde_json::to_value(account).unwrap();
        assert_eq!(value["status"], "takendown");
        assert_eq!(AccountStatus::Takendown.to_string(), "Takendown");
    }

    #[test]
    fn sync_serializes_blocks_as_cbor_bytes() {
        let sync = SubscribeReposSync {
            seq: 1,
            did: "did:plc:test".to_string(),
            blocks: vec![1, 2, 3],
            rev: "3jzfcijpj2z2a".to_string(),
            time: test_time(),
        };
        let bytes = serde_ipld_dagcbor::to_vec(&sync).unwrap();
        let decoded: ipld_core::ipld::Ipld = serde_ipld_dagcbor::from_slice(&bytes).unwrap();
        let ipld_core::ipld::Ipld::Map(map) = decoded else {
            panic!("expected map");
        };
        assert!(matches!(
            map.get("blocks"),
            Some(ipld_core::ipld::Ipld::Bytes(b)) if b == &vec![1u8, 2, 3]
        ));
        assert_eq!(
            map.get("time"),
            Some(&ipld_core::ipld::Ipld::String(
                "2026-01-12T19:45:23.307Z".to_string()
            ))
        );
    }

    #[test]
    fn commit_round_trips_through_dag_cbor() {
        let mut commit = test_commit();
        commit.prev_data = Some(Cid::from_str(TEST_CID).unwrap());
        let bytes = serde_ipld_dagcbor::to_vec(&commit).unwrap();
        let decoded: SubscribeReposCommit = serde_ipld_dagcbor::from_slice(&bytes).unwrap();
        assert_eq!(decoded.seq, commit.seq);
        assert_eq!(decoded.prev, None);
        assert_eq!(decoded.prev_data, commit.prev_data);
        assert_eq!(decoded.ops[0].prev, None);
        assert_eq!(decoded.blocks, commit.blocks);
    }

    const BLOB_CID_A: &str = "bafkreihdwdcefgh4dqkjv67uzcmw7ojee6xedzdetojuzjevtenxquvyku";
    const BLOB_CID_B: &str = "bafkreie5cvv4h45feadgeuwhbcutmh6t2ceseocckahdoe6uat64zmz454";

    /// Hand-build a DAG-CBOR `#commit` frame body whose `blobs` array holds the given
    /// elements, so the test controls the exact wire encoding of each element.
    fn commit_body_with_blobs(blobs: Vec<ipld_core::ipld::Ipld>) -> Vec<u8> {
        use ipld_core::ipld::Ipld;
        use std::collections::BTreeMap;

        let mut op = BTreeMap::new();
        op.insert(
            "path".to_string(),
            Ipld::String("app.bsky.feed.post/3jzfcijpj2z2a".to_string()),
        );
        op.insert("action".to_string(), Ipld::String("create".to_string()));
        op.insert(
            "cid".to_string(),
            Ipld::Link(Cid::from_str(TEST_CID).unwrap()),
        );

        let mut body = BTreeMap::new();
        body.insert("seq".to_string(), Ipld::Integer(42));
        body.insert(
            "time".to_string(),
            Ipld::String("2026-01-12T19:45:23.307Z".to_string()),
        );
        body.insert("rebase".to_string(), Ipld::Bool(false));
        body.insert("tooBig".to_string(), Ipld::Bool(false));
        body.insert("repo".to_string(), Ipld::String("did:plc:test".to_string()));
        body.insert(
            "commit".to_string(),
            Ipld::Link(Cid::from_str(TEST_CID).unwrap()),
        );
        body.insert("rev".to_string(), Ipld::String("3jzfcijpj2z2a".to_string()));
        body.insert("since".to_string(), Ipld::Null);
        body.insert("blocks".to_string(), Ipld::Bytes(vec![1, 2, 3]));
        body.insert("ops".to_string(), Ipld::List(vec![Ipld::Map(op)]));
        body.insert("blobs".to_string(), Ipld::List(blobs));
        serde_ipld_dagcbor::to_vec(&Ipld::Map(body)).unwrap()
    }

    /// The lexicon defines `blobs` as `array` of `cid-link`, which DAG-CBOR encodes as
    /// tag 42 (0xD8 0x2A) wrapping the binary CID. The `tranquil` PDS populates this
    /// field; decoding it as text used to fail with `Mismatch { expect_major: 3, byte: 216 }`.
    #[test]
    fn commit_decodes_blobs_encoded_as_cid_links() {
        use ipld_core::ipld::Ipld;

        let a = Cid::from_str(BLOB_CID_A).unwrap();
        let b = Cid::from_str(BLOB_CID_B).unwrap();
        let body = commit_body_with_blobs(vec![Ipld::Link(a), Ipld::Link(b)]);
        let decoded: SubscribeReposCommit =
            serde_ipld_dagcbor::from_slice(&body).expect("commit with tag-42 blobs must decode");
        assert_eq!(decoded.seq, 42);
        assert_eq!(decoded.blobs.len(), 2);
        assert_eq!(decoded.blobs[0].to_string(), BLOB_CID_A);
        assert_eq!(decoded.blobs[1].to_string(), BLOB_CID_B);
    }

    /// rsky-pds (non-conformantly) emitted `blobs` as text-string CIDs; those frames exist
    /// in the wild and must keep decoding.
    #[test]
    fn commit_decodes_blobs_encoded_as_text_strings() {
        use ipld_core::ipld::Ipld;

        let body = commit_body_with_blobs(vec![
            Ipld::String(BLOB_CID_A.to_string()),
            Ipld::String(BLOB_CID_B.to_string()),
        ]);
        let decoded: SubscribeReposCommit = serde_ipld_dagcbor::from_slice(&body)
            .expect("commit with text-string blobs must decode");
        assert_eq!(decoded.blobs.len(), 2);
        assert_eq!(decoded.blobs[0].to_string(), BLOB_CID_A);
        assert_eq!(decoded.blobs[1].to_string(), BLOB_CID_B);
    }

    #[test]
    fn commit_decodes_missing_blobs_as_empty() {
        use ipld_core::ipld::Ipld;

        let mut body = commit_body_with_blobs(vec![]);
        // Re-encode without the `blobs` key at all.
        let Ipld::Map(mut map) = serde_ipld_dagcbor::from_slice::<Ipld>(&body).unwrap() else {
            panic!("expected map");
        };
        map.remove("blobs");
        body = serde_ipld_dagcbor::to_vec(&Ipld::Map(map)).unwrap();
        let decoded: SubscribeReposCommit = serde_ipld_dagcbor::from_slice(&body).unwrap();
        assert!(decoded.blobs.is_empty());
    }

    #[test]
    fn commit_round_trips_blobs_as_cid_links() {
        use ipld_core::ipld::Ipld;

        let a = Cid::from_str(BLOB_CID_A).unwrap();
        let b = Cid::from_str(BLOB_CID_B).unwrap();
        let mut commit = test_commit();
        commit.blobs = vec![a, b];
        let bytes = serde_ipld_dagcbor::to_vec(&commit).unwrap();

        // Each blob must be on the wire as a CID link (tag 42 wrapping 0x00 || cid bytes).
        for cid in [&a, &b] {
            let mut link = vec![0xD8, 0x2A];
            let mut cid_bytes = vec![MULTIBASE_IDENTITY];
            cid_bytes.extend(cid.to_bytes());
            link.push(0x58); // bytes, 1-byte length follows
            link.push(cid_bytes.len() as u8);
            link.extend(cid_bytes);
            assert!(
                bytes.windows(link.len()).any(|w| w == link.as_slice()),
                "serialized commit must contain tag-42 link for {cid}"
            );
        }
        let Ipld::Map(map) = serde_ipld_dagcbor::from_slice::<Ipld>(&bytes).unwrap() else {
            panic!("expected map");
        };
        assert_eq!(
            map.get("blobs"),
            Some(&Ipld::List(vec![Ipld::Link(a), Ipld::Link(b)]))
        );

        let decoded: SubscribeReposCommit = serde_ipld_dagcbor::from_slice(&bytes).unwrap();
        assert_eq!(decoded.blobs, commit.blobs);
        assert_eq!(decoded.commit, commit.commit);
        assert_eq!(decoded.blocks, commit.blocks);
    }
}
