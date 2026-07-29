#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Policy {
    Public,
    MemberList,
    ManagingApp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "$type")]
pub enum AppAccess {
    #[serde(rename = "com.atproto.simplespace.defs#appAccessOpen")]
    Open(AppAccessOpen),
    #[serde(rename = "com.atproto.simplespace.defs#appAccessAllowList")]
    AllowList(AppAccessAllowList),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppAccessOpen {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppAccessAllowList {
    pub allowed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy: Option<Policy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_access: Option<AppAccess>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub managing_app: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSpaceInput {
    pub space_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<Config>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSpaceOutput {
    pub space: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateSpaceInput {
    pub space: String,
    pub config: Config,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteSpaceInput {
    pub space: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddMemberInput {
    pub space: String,
    pub did: String,
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
pub struct Member {
    pub did: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListMembersOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub members: Vec<Member>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckUserAccessParams {
    pub space: String,
    pub did: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckUserAccessOutput {
    pub allowed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
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
        assert_eq!(&value.clone(), value);
        assert!(!format!("{value:?}").is_empty());
    }

    #[test]
    fn policy_values_match_lexicon_known_values() {
        roundtrip(&Policy::Public, r#""public""#);
        roundtrip(&Policy::MemberList, r#""member-list""#);
        roundtrip(&Policy::ManagingApp, r#""managing-app""#);
    }

    #[test]
    fn app_access_union_variants() {
        roundtrip(
            &AppAccess::Open(AppAccessOpen {}),
            r#"{"$type":"com.atproto.simplespace.defs#appAccessOpen"}"#,
        );
        roundtrip(
            &AppAccess::AllowList(AppAccessAllowList {
                allowed: vec!["https://app.example.com/client-metadata.json".to_string()],
            }),
            r#"{"$type":"com.atproto.simplespace.defs#appAccessAllowList","allowed":["https://app.example.com/client-metadata.json"]}"#,
        );
    }

    #[test]
    fn config_omits_absent_fields() {
        roundtrip(
            &Config {
                policy: None,
                app_access: None,
                managing_app: None,
            },
            r#"{}"#,
        );
        roundtrip(
            &Config {
                policy: Some(Policy::ManagingApp),
                app_access: Some(AppAccess::AllowList(AppAccessAllowList {
                    allowed: vec!["https://app.example.com/client-metadata.json".to_string()],
                })),
                managing_app: Some("did:web:app.example.com#atmoboards".to_string()),
            },
            r#"{"policy":"managing-app","appAccess":{"$type":"com.atproto.simplespace.defs#appAccessAllowList","allowed":["https://app.example.com/client-metadata.json"]},"managingApp":"did:web:app.example.com#atmoboards"}"#,
        );
    }

    #[test]
    fn create_space_pair() {
        roundtrip(
            &CreateSpaceInput {
                space_type: "com.example.forum".to_string(),
                skey: Some("self".to_string()),
                config: Some(Config {
                    policy: Some(Policy::Public),
                    app_access: None,
                    managing_app: None,
                }),
            },
            r#"{"spaceType":"com.example.forum","skey":"self","config":{"policy":"public"}}"#,
        );
        roundtrip(
            &CreateSpaceOutput {
                space: SPACE.to_string(),
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self"}"#,
        );
    }

    #[test]
    fn space_management_inputs() {
        roundtrip(
            &UpdateSpaceInput {
                space: SPACE.to_string(),
                config: Config {
                    policy: Some(Policy::MemberList),
                    app_access: None,
                    managing_app: None,
                },
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self","config":{"policy":"member-list"}}"#,
        );
        roundtrip(
            &DeleteSpaceInput {
                space: SPACE.to_string(),
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self"}"#,
        );
    }

    #[test]
    fn membership_inputs() {
        roundtrip(
            &AddMemberInput {
                space: SPACE.to_string(),
                did: "did:plc:member".to_string(),
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self","did":"did:plc:member"}"#,
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
                }],
            },
            r#"{"cursor":"c1","members":[{"did":"did:plc:member"}]}"#,
        );
    }

    #[test]
    fn check_user_access_pair() {
        roundtrip(
            &CheckUserAccessParams {
                space: SPACE.to_string(),
                did: "did:plc:member".to_string(),
                client_id: Some("https://app.example.com/client-metadata.json".to_string()),
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self","did":"did:plc:member","clientId":"https://app.example.com/client-metadata.json"}"#,
        );
        roundtrip(
            &CheckUserAccessOutput {
                allowed: false,
                reason: Some("not a follower".to_string()),
            },
            r#"{"allowed":false,"reason":"not a follower"}"#,
        );
    }
}
