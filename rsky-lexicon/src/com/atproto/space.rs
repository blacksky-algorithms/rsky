use crate::com::atproto::simplespace::Config as SimplespaceConfig;
use chrono::{DateTime, Utc};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedCommit {
    pub ver: i64,
    #[serde(with = "crate::atproto_bytes")]
    pub hash: Vec<u8>,
    #[serde(with = "crate::atproto_bytes")]
    pub ikm: Vec<u8>,
    #[serde(with = "crate::atproto_bytes")]
    pub sig: Vec<u8>,
    #[serde(with = "crate::atproto_bytes")]
    pub mac: Vec<u8>,
    pub rev: String,
}

/// Absent or null `cid` means a delete; absent `prev` means a create.
/// Operations applied atomically share a `rev`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepoOp {
    pub rev: String,
    pub collection: String,
    pub rkey: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoRef {
    pub did: String,
    pub rev: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitMeta {
    pub rev: String,
    pub hash: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "$type")]
pub enum SpaceConfig {
    #[serde(rename = "com.atproto.simplespace.defs#config")]
    Simplespace(SimplespaceConfig),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetSpaceParams {
    pub space: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GetSpaceOutput {
    pub space: String,
    pub config: SpaceConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetSpaceCredentialInput {
    pub space: String,
    pub delegation_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_attestation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetSpaceCredentialOutput {
    pub credential: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListReposParams {
    pub space: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListReposOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub repos: Vec<RepoRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetRecordParams {
    pub space: String,
    pub did: String,
    pub collection: String,
    pub rkey: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GetRecordOutput {
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRecordsParams {
    pub space: String,
    pub did: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_values: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub uri: String,
    pub cid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListRecordsOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub records: Vec<Record>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetBlobParams {
    pub space: String,
    pub did: String,
    pub cid: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetLatestCommitParams {
    pub space: String,
    pub did: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetLatestCommitOutput {
    pub commit: SignedCommit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetRepoParams {
    pub space: String,
    pub did: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRepoOpsParams {
    pub space: String,
    pub did: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_values: Option<bool>,
}

/// `commit` is present if and only if the page includes the last available
/// operation for the repo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListRepoOpsOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub ops: Vec<RepoOp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<SignedCommit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetDelegationTokenParams {
    pub space: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetDelegationTokenOutput {
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateRecordInput {
    pub space: String,
    pub collection: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validate: Option<bool>,
    pub record: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateRecordOutput {
    pub uri: String,
    pub cid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<CommitMeta>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PutRecordInput {
    pub space: String,
    pub collection: String,
    pub rkey: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validate: Option<bool>,
    pub record: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swap_record: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PutRecordOutput {
    pub uri: String,
    pub cid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<CommitMeta>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteRecordInput {
    pub space: String,
    pub collection: String,
    pub rkey: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swap_record: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteRecordOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<CommitMeta>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplyWritesInput {
    pub space: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validate: Option<bool>,
    pub writes: Vec<ApplyWritesWrite>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "$type")]
pub enum ApplyWritesWrite {
    #[serde(rename = "com.atproto.space.applyWrites#create")]
    Create(WriteCreate),
    #[serde(rename = "com.atproto.space.applyWrites#update")]
    Update(WriteUpdate),
    #[serde(rename = "com.atproto.space.applyWrites#delete")]
    Delete(WriteDelete),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WriteCreate {
    pub collection: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rkey: Option<String>,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WriteUpdate {
    pub collection: String,
    pub rkey: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteDelete {
    pub collection: String,
    pub rkey: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplyWritesOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<CommitMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub results: Option<Vec<ApplyWritesResult>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "$type")]
pub enum ApplyWritesResult {
    #[serde(rename = "com.atproto.space.applyWrites#createResult")]
    Create(CreateResult),
    #[serde(rename = "com.atproto.space.applyWrites#updateResult")]
    Update(UpdateResult),
    #[serde(rename = "com.atproto.space.applyWrites#deleteResult")]
    Delete(DeleteResult),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateResult {
    pub uri: String,
    pub cid: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateResult {
    pub uri: String,
    pub cid: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteResult {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListSpacesParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListSpacesOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub spaces: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterNotifyInput {
    pub space: String,
    pub endpoint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterNotifyOutput {
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifyWriteInput {
    pub space: String,
    pub did: String,
    pub rev: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifySpaceDeletedInput {
    pub space: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::com::atproto::simplespace::{AppAccess, AppAccessOpen, Policy};
    use chrono::TimeZone;
    use serde::de::DeserializeOwned;
    use serde::Serialize;
    use serde_json::json;
    use std::fmt::Debug;

    const SPACE: &str = "at://did:plc:auth/space/com.example.forum/self";

    fn roundtrip<T>(value: &T, expected: &str)
    where
        T: Serialize + DeserializeOwned + PartialEq + Debug + Clone,
    {
        assert_eq!(serde_json::to_string(value).unwrap(), expected);
        assert_eq!(&serde_json::from_str::<T>(expected).unwrap(), value);
        assert_eq!(&value.clone(), value);
        assert!(!format!("{value:?}").is_empty());
    }

    fn signed_commit() -> SignedCommit {
        SignedCommit {
            ver: 1,
            hash: vec![1, 2, 3],
            ikm: vec![4, 5, 6],
            sig: vec![7, 8, 9],
            mac: vec![10, 11, 12],
            rev: "3jzfcijpj2z2a".to_string(),
        }
    }

    #[test]
    fn signed_commit_json_is_byte_exact() {
        roundtrip(
            &signed_commit(),
            r#"{"ver":1,"hash":{"$bytes":"AQID"},"ikm":{"$bytes":"BAUG"},"sig":{"$bytes":"BwgJ"},"mac":{"$bytes":"CgsM"},"rev":"3jzfcijpj2z2a"}"#,
        );
    }

    #[test]
    fn signed_commit_deserializes_from_seq() {
        let commit: SignedCommit = serde_json::from_str(
            r#"[1,{"$bytes":"AQID"},{"$bytes":"BAUG"},{"$bytes":"BwgJ"},{"$bytes":"CgsM"},"3jzfcijpj2z2a"]"#,
        )
        .unwrap();
        assert_eq!(commit, signed_commit());
    }

    #[test]
    fn signed_commit_cbor_roundtrips_raw_bytes() {
        let commit = signed_commit();
        let bytes = serde_cbor::to_vec(&commit).unwrap();
        assert_eq!(
            serde_cbor::from_slice::<SignedCommit>(&bytes).unwrap(),
            commit
        );
    }

    #[test]
    fn repo_op_delete_omits_cid_entirely() {
        let op = RepoOp {
            rev: "3jzfcijpj2z2a".to_string(),
            collection: "com.example.post".to_string(),
            rkey: "3jzfcijpj2z2b".to_string(),
            cid: None,
            prev: Some("bafyreiaold".to_string()),
            value: None,
        };
        roundtrip(
            &op,
            r#"{"rev":"3jzfcijpj2z2a","collection":"com.example.post","rkey":"3jzfcijpj2z2b","prev":"bafyreiaold"}"#,
        );
        assert!(op.cid.is_none());
    }

    #[test]
    fn repo_op_accepts_null_cid_for_delete() {
        let op: RepoOp = serde_json::from_str(
            r#"{"rev":"3jzfcijpj2z2a","collection":"com.example.post","rkey":"3jzfcijpj2z2b","cid":null,"prev":"bafyreiaold"}"#,
        )
        .unwrap();
        assert!(op.cid.is_none());
    }

    #[test]
    fn repo_op_create_inlines_value() {
        roundtrip(
            &RepoOp {
                rev: "3jzfcijpj2z2a".to_string(),
                collection: "com.example.post".to_string(),
                rkey: "3jzfcijpj2z2b".to_string(),
                cid: Some("bafyreianew".to_string()),
                prev: None,
                value: Some(json!({"text": "hi"})),
            },
            r#"{"rev":"3jzfcijpj2z2a","collection":"com.example.post","rkey":"3jzfcijpj2z2b","cid":"bafyreianew","value":{"text":"hi"}}"#,
        );
    }

    #[test]
    fn repo_ref_omits_absent_hash() {
        roundtrip(
            &RepoRef {
                did: "did:plc:writer".to_string(),
                rev: "3jzfcijpj2z2a".to_string(),
                hash: None,
            },
            r#"{"did":"did:plc:writer","rev":"3jzfcijpj2z2a"}"#,
        );
        roundtrip(
            &RepoRef {
                did: "did:plc:writer".to_string(),
                rev: "3jzfcijpj2z2a".to_string(),
                hash: Some("ab12".to_string()),
            },
            r#"{"did":"did:plc:writer","rev":"3jzfcijpj2z2a","hash":"ab12"}"#,
        );
    }

    #[test]
    fn get_space_pair() {
        roundtrip(
            &GetSpaceParams {
                space: SPACE.to_string(),
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self"}"#,
        );
        roundtrip(
            &GetSpaceOutput {
                space: SPACE.to_string(),
                config: SpaceConfig::Simplespace(SimplespaceConfig {
                    policy: Some(Policy::MemberList),
                    app_access: Some(AppAccess::Open(AppAccessOpen {})),
                    managing_app: None,
                }),
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self","config":{"$type":"com.atproto.simplespace.defs#config","policy":"member-list","appAccess":{"$type":"com.atproto.simplespace.defs#appAccessOpen"}}}"#,
        );
    }

    #[test]
    fn get_space_credential_pair() {
        roundtrip(
            &GetSpaceCredentialInput {
                space: SPACE.to_string(),
                delegation_token: "dt.jwt".to_string(),
                client_attestation: Some("ca.jwt".to_string()),
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self","delegationToken":"dt.jwt","clientAttestation":"ca.jwt"}"#,
        );
        roundtrip(
            &GetSpaceCredentialOutput {
                credential: "sc.jwt".to_string(),
            },
            r#"{"credential":"sc.jwt"}"#,
        );
    }

    #[test]
    fn list_repos_pair() {
        roundtrip(
            &ListReposParams {
                space: SPACE.to_string(),
                limit: Some(500),
                cursor: None,
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self","limit":500}"#,
        );
        roundtrip(
            &ListReposOutput {
                cursor: Some("c1".to_string()),
                repos: vec![RepoRef {
                    did: "did:plc:writer".to_string(),
                    rev: "3jzfcijpj2z2a".to_string(),
                    hash: None,
                }],
            },
            r#"{"cursor":"c1","repos":[{"did":"did:plc:writer","rev":"3jzfcijpj2z2a"}]}"#,
        );
    }

    #[test]
    fn get_record_pair() {
        roundtrip(
            &GetRecordParams {
                space: SPACE.to_string(),
                did: "did:plc:writer".to_string(),
                collection: "com.example.post".to_string(),
                rkey: "3jzfcijpj2z2b".to_string(),
                cid: None,
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self","did":"did:plc:writer","collection":"com.example.post","rkey":"3jzfcijpj2z2b"}"#,
        );
        roundtrip(
            &GetRecordOutput {
                uri: format!("{SPACE}/did:plc:writer/com.example.post/3jzfcijpj2z2b"),
                cid: Some("bafyreianew".to_string()),
                value: json!({"text": "hi"}),
            },
            r#"{"uri":"at://did:plc:auth/space/com.example.forum/self/did:plc:writer/com.example.post/3jzfcijpj2z2b","cid":"bafyreianew","value":{"text":"hi"}}"#,
        );
    }

    #[test]
    fn list_records_pair() {
        roundtrip(
            &ListRecordsParams {
                space: SPACE.to_string(),
                did: "did:plc:writer".to_string(),
                collection: None,
                limit: Some(50),
                cursor: None,
                exclude_values: Some(true),
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self","did":"did:plc:writer","limit":50,"excludeValues":true}"#,
        );
        roundtrip(
            &ListRecordsOutput {
                cursor: None,
                records: vec![Record {
                    uri: format!("{SPACE}/did:plc:writer/com.example.post/3jzfcijpj2z2b"),
                    cid: "bafyreianew".to_string(),
                    value: None,
                }],
            },
            r#"{"records":[{"uri":"at://did:plc:auth/space/com.example.forum/self/did:plc:writer/com.example.post/3jzfcijpj2z2b","cid":"bafyreianew"}]}"#,
        );
    }

    #[test]
    fn car_and_blob_params() {
        roundtrip(
            &GetBlobParams {
                space: SPACE.to_string(),
                did: "did:plc:writer".to_string(),
                cid: "bafkreiblob".to_string(),
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self","did":"did:plc:writer","cid":"bafkreiblob"}"#,
        );
        roundtrip(
            &GetRepoParams {
                space: SPACE.to_string(),
                did: "did:plc:writer".to_string(),
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self","did":"did:plc:writer"}"#,
        );
    }

    #[test]
    fn get_latest_commit_pair() {
        roundtrip(
            &GetLatestCommitParams {
                space: SPACE.to_string(),
                did: "did:plc:writer".to_string(),
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self","did":"did:plc:writer"}"#,
        );
        roundtrip(
            &GetLatestCommitOutput {
                commit: signed_commit(),
            },
            r#"{"commit":{"ver":1,"hash":{"$bytes":"AQID"},"ikm":{"$bytes":"BAUG"},"sig":{"$bytes":"BwgJ"},"mac":{"$bytes":"CgsM"},"rev":"3jzfcijpj2z2a"}}"#,
        );
    }

    #[test]
    fn list_repo_ops_pair() {
        roundtrip(
            &ListRepoOpsParams {
                space: SPACE.to_string(),
                did: "did:plc:writer".to_string(),
                since: Some("3jzfcijpj2z2a".to_string()),
                cursor: None,
                limit: Some(500),
                exclude_values: None,
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self","did":"did:plc:writer","since":"3jzfcijpj2z2a","limit":500}"#,
        );
        roundtrip(
            &ListRepoOpsOutput {
                cursor: None,
                ops: vec![RepoOp {
                    rev: "3jzfcijpj2z2c".to_string(),
                    collection: "com.example.post".to_string(),
                    rkey: "3jzfcijpj2z2b".to_string(),
                    cid: Some("bafyreianew".to_string()),
                    prev: None,
                    value: None,
                }],
                commit: Some(signed_commit()),
            },
            r#"{"ops":[{"rev":"3jzfcijpj2z2c","collection":"com.example.post","rkey":"3jzfcijpj2z2b","cid":"bafyreianew"}],"commit":{"ver":1,"hash":{"$bytes":"AQID"},"ikm":{"$bytes":"BAUG"},"sig":{"$bytes":"BwgJ"},"mac":{"$bytes":"CgsM"},"rev":"3jzfcijpj2z2a"}}"#,
        );
    }

    #[test]
    fn get_delegation_token_pair() {
        roundtrip(
            &GetDelegationTokenParams {
                space: SPACE.to_string(),
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self"}"#,
        );
        roundtrip(
            &GetDelegationTokenOutput {
                token: "dt.jwt".to_string(),
            },
            r#"{"token":"dt.jwt"}"#,
        );
    }

    #[test]
    fn create_record_pair() {
        roundtrip(
            &CreateRecordInput {
                space: SPACE.to_string(),
                collection: "com.example.post".to_string(),
                rkey: None,
                validate: Some(true),
                record: json!({"text": "hi"}),
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self","collection":"com.example.post","validate":true,"record":{"text":"hi"}}"#,
        );
        roundtrip(
            &CreateRecordOutput {
                uri: format!("{SPACE}/did:plc:writer/com.example.post/3jzfcijpj2z2b"),
                cid: "bafyreianew".to_string(),
                commit: Some(CommitMeta {
                    rev: "3jzfcijpj2z2c".to_string(),
                    hash: "ab12".to_string(),
                }),
            },
            r#"{"uri":"at://did:plc:auth/space/com.example.forum/self/did:plc:writer/com.example.post/3jzfcijpj2z2b","cid":"bafyreianew","commit":{"rev":"3jzfcijpj2z2c","hash":"ab12"}}"#,
        );
    }

    #[test]
    fn put_record_pair() {
        roundtrip(
            &PutRecordInput {
                space: SPACE.to_string(),
                collection: "com.example.post".to_string(),
                rkey: "3jzfcijpj2z2b".to_string(),
                validate: None,
                record: json!({"text": "hi"}),
                swap_record: Some("bafyreiaold".to_string()),
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self","collection":"com.example.post","rkey":"3jzfcijpj2z2b","record":{"text":"hi"},"swapRecord":"bafyreiaold"}"#,
        );
        roundtrip(
            &PutRecordOutput {
                uri: format!("{SPACE}/did:plc:writer/com.example.post/3jzfcijpj2z2b"),
                cid: "bafyreianew".to_string(),
                commit: None,
            },
            r#"{"uri":"at://did:plc:auth/space/com.example.forum/self/did:plc:writer/com.example.post/3jzfcijpj2z2b","cid":"bafyreianew"}"#,
        );
    }

    #[test]
    fn delete_record_pair() {
        roundtrip(
            &DeleteRecordInput {
                space: SPACE.to_string(),
                collection: "com.example.post".to_string(),
                rkey: "3jzfcijpj2z2b".to_string(),
                swap_record: None,
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self","collection":"com.example.post","rkey":"3jzfcijpj2z2b"}"#,
        );
        roundtrip(
            &DeleteRecordOutput {
                commit: Some(CommitMeta {
                    rev: "3jzfcijpj2z2c".to_string(),
                    hash: "ab12".to_string(),
                }),
            },
            r#"{"commit":{"rev":"3jzfcijpj2z2c","hash":"ab12"}}"#,
        );
    }

    #[test]
    fn apply_writes_pair() {
        roundtrip(
            &ApplyWritesInput {
                space: SPACE.to_string(),
                validate: None,
                writes: vec![
                    ApplyWritesWrite::Create(WriteCreate {
                        collection: "com.example.post".to_string(),
                        rkey: None,
                        value: json!({"text": "hi"}),
                    }),
                    ApplyWritesWrite::Update(WriteUpdate {
                        collection: "com.example.post".to_string(),
                        rkey: "3jzfcijpj2z2b".to_string(),
                        value: json!({"text": "hello"}),
                    }),
                    ApplyWritesWrite::Delete(WriteDelete {
                        collection: "com.example.post".to_string(),
                        rkey: "3jzfcijpj2z2d".to_string(),
                    }),
                ],
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self","writes":[{"$type":"com.atproto.space.applyWrites#create","collection":"com.example.post","value":{"text":"hi"}},{"$type":"com.atproto.space.applyWrites#update","collection":"com.example.post","rkey":"3jzfcijpj2z2b","value":{"text":"hello"}},{"$type":"com.atproto.space.applyWrites#delete","collection":"com.example.post","rkey":"3jzfcijpj2z2d"}]}"#,
        );
        roundtrip(
            &ApplyWritesOutput {
                commit: Some(CommitMeta {
                    rev: "3jzfcijpj2z2c".to_string(),
                    hash: "ab12".to_string(),
                }),
                results: Some(vec![
                    ApplyWritesResult::Create(CreateResult {
                        uri: "u1".to_string(),
                        cid: "c1".to_string(),
                    }),
                    ApplyWritesResult::Update(UpdateResult {
                        uri: "u2".to_string(),
                        cid: "c2".to_string(),
                    }),
                    ApplyWritesResult::Delete(DeleteResult {}),
                ]),
            },
            r#"{"commit":{"rev":"3jzfcijpj2z2c","hash":"ab12"},"results":[{"$type":"com.atproto.space.applyWrites#createResult","uri":"u1","cid":"c1"},{"$type":"com.atproto.space.applyWrites#updateResult","uri":"u2","cid":"c2"},{"$type":"com.atproto.space.applyWrites#deleteResult"}]}"#,
        );
    }

    #[test]
    fn list_spaces_pair() {
        roundtrip(
            &ListSpacesParams {
                limit: None,
                cursor: Some("c1".to_string()),
            },
            r#"{"cursor":"c1"}"#,
        );
        roundtrip(
            &ListSpacesOutput {
                cursor: None,
                spaces: vec![SPACE.to_string()],
            },
            r#"{"spaces":["at://did:plc:auth/space/com.example.forum/self"]}"#,
        );
    }

    #[test]
    fn register_notify_pair() {
        roundtrip(
            &RegisterNotifyInput {
                space: SPACE.to_string(),
                endpoint: "https://sync.example.com".to_string(),
                repo: Some("did:plc:writer".to_string()),
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self","endpoint":"https://sync.example.com","repo":"did:plc:writer"}"#,
        );
        roundtrip(
            &RegisterNotifyOutput {
                expires_at: chrono::Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).unwrap(),
            },
            r#"{"expiresAt":"2030-01-01T00:00:00Z"}"#,
        );
    }

    #[test]
    fn notify_inputs() {
        roundtrip(
            &NotifyWriteInput {
                space: SPACE.to_string(),
                did: "did:plc:writer".to_string(),
                rev: "3jzfcijpj2z2c".to_string(),
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self","did":"did:plc:writer","rev":"3jzfcijpj2z2c"}"#,
        );
        roundtrip(
            &NotifySpaceDeletedInput {
                space: SPACE.to_string(),
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self"}"#,
        );
    }
}
