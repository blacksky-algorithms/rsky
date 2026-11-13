//! `com.atproto.memberspace.*` types: the baseline policy for a space governed
//! by a member list maintained by the PDS.

/// `com.atproto.memberspace.policy`. Omitted booleans default to false; an
/// omitted `allowedApps` means any application may read the space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Policy {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_read: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_write: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_apps: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publish_policy: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publish_members: Option<bool>,
}

/// A member-list entry: required `read` and `write` booleans, which
/// `putMember` replaces together. `memberRev`/`addedAt` are rsky extensions
/// carried as optional extra properties.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub did: String,
    pub read: bool,
    pub write: bool,
    #[serde(rename = "memberRev", skip_serializing_if = "Option::is_none")]
    pub member_rev: Option<String>,
    #[serde(rename = "addedAt", skip_serializing_if = "Option::is_none")]
    pub added_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PutMemberInput {
    pub space: String,
    pub did: String,
    pub read: bool,
    pub write: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoveMemberInput {
    pub space: String,
    pub did: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListMembersParams {
    pub space: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListMembersOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub members: Vec<Member>,
}

/// `com.atproto.memberspace.member`: the record the PDS publishes into the
/// authority's own repo when the policy sets `publishMembers`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberRecord {
    pub read: bool,
    pub write: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::DeserializeOwned;
    use serde::Serialize;
    use std::fmt::Debug;

    const SPACE: &str = "at://did:plc:auth/space/com.example.forum/self";

    fn roundtrip<T>(value: &T, expected: &str)
    where
        T: Serialize + DeserializeOwned + PartialEq + Debug + Clone,
    {
        assert_eq!(serde_json::to_string(value).unwrap(), expected);
        assert_eq!(&serde_json::from_str::<T>(expected).unwrap(), value);
    }

    #[test]
    fn policy_omits_absent_fields() {
        roundtrip(&Policy::default(), r#"{}"#);
        roundtrip(
            &Policy {
                public_read: Some(true),
                public_write: None,
                allowed_apps: Some(vec!["https://app.example/client-metadata.json".to_string()]),
                publish_policy: Some(true),
                publish_members: Some(false),
            },
            r#"{"publicRead":true,"allowedApps":["https://app.example/client-metadata.json"],"publishPolicy":true,"publishMembers":false}"#,
        );
    }

    #[test]
    fn member_carries_read_and_write() {
        roundtrip(
            &Member {
                did: "did:plc:member".to_string(),
                read: true,
                write: false,
                member_rev: Some("3jzfcijpj2z2a".to_string()),
                added_at: Some("2026-01-01T00:00:00.000Z".to_string()),
            },
            r#"{"did":"did:plc:member","read":true,"write":false,"memberRev":"3jzfcijpj2z2a","addedAt":"2026-01-01T00:00:00.000Z"}"#,
        );
    }

    #[test]
    fn membership_inputs() {
        roundtrip(
            &PutMemberInput {
                space: SPACE.to_string(),
                did: "did:plc:member".to_string(),
                read: true,
                write: true,
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self","did":"did:plc:member","read":true,"write":true}"#,
        );
        roundtrip(
            &RemoveMemberInput {
                space: SPACE.to_string(),
                did: "did:plc:member".to_string(),
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self","did":"did:plc:member"}"#,
        );
    }

    #[test]
    fn list_members_pair() {
        roundtrip(
            &ListMembersParams {
                space: SPACE.to_string(),
                limit: Some(500),
                cursor: None,
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self","limit":500}"#,
        );
        roundtrip(
            &ListMembersOutput {
                cursor: Some("c1".to_string()),
                members: vec![Member {
                    did: "did:plc:member".to_string(),
                    read: true,
                    write: true,
                    member_rev: None,
                    added_at: None,
                }],
            },
            r#"{"cursor":"c1","members":[{"did":"did:plc:member","read":true,"write":true}]}"#,
        );
    }
}
