//! # DEPRECATED - Backward Compatibility Only
//!
//! This module contains legacy types for the deprecated
//! `com.atproto.simplespace.*` API namespace.
//!
//! **DO NOT USE IN NEW CODE.**
//!
//! These types exist solely to maintain backward compatibility
//! with applications that haven't migrated to the new space APIs:
//! - `com.atproto.space.*`
//! - `com.atproto.memberspace.*`
//! - `com.atproto.managedspace.*`
//! - `com.atproto.personalspace.*`
//!
//! **Support Status:** Indefinite backward compatibility support
//!
//! This API will continue to work indefinitely. Client applications
//! are encouraged to migrate to the new APIs for better feature support.
//!
//! See implementation plan: `20261104_0537_PLAN.md`
//! See breaking changes review: `20261103_1245_REVIEW.md`

use serde::{Deserialize, Serialize};

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

/// Space configuration for the legacy simplespace API.
///
/// Note: `policy` is stored as a string ("public", "member-list", "managing-app")
/// to match the original JSON schema.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    /// Policy type: "public", "member-list", or "managing-app"
    /// Defaults to None -> "member-list" in config_to_policy
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_access: Option<AppAccess>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub managing_app: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSpaceInput {
    #[serde(rename = "type")]
    pub space_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<Config>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSpaceOutput {
    pub uri: String,
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

/// Legacy member representation.
///
/// Note: `member_rev` and `added_at` are required strings (not Option)
/// in the legacy API. Empty strings are used for rows that predate the columns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub did: String,
    /// Rev (TID) at which this member was added. Empty for rows that predate
    /// the column.
    #[serde(rename = "memberRev")]
    pub member_rev: String,
    /// ISO-8601 add timestamp. Empty for rows that predate the column.
    #[serde(rename = "addedAt")]
    pub added_at: String,
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
                policy: Some("managing-app".to_string()),
                app_access: Some(AppAccess::AllowList(AppAccessAllowList {
                    allowed: vec!["https://app.example.com/client-metadata.json".to_string()],
                })),
                managing_app: Some("did:web:app.example.com#atmoboards".to_string()),
            },
            r#"{"policy":"managing-app","appAccess":{"$type":"com.atproto.simplespace.defs#appAccessAllowList","allowed":["https://app.example.com/client-metadata.json"]},"managingApp":"did:web:app.example.com#atmoboards"}"#,
        );
    }

    #[test]
    fn config_default_is_empty() {
        let config = Config::default();
        assert_eq!(config.policy, None);
        assert_eq!(config.app_access, None);
        assert_eq!(config.managing_app, None);
    }

    #[test]
    fn create_space_pair() {
        roundtrip(
            &CreateSpaceInput {
                space_type: "com.example.forum".to_string(),
                skey: Some("self".to_string()),
                config: Some(Config {
                    policy: Some("public".to_string()),
                    app_access: None,
                    managing_app: None,
                }),
            },
            r#"{"type":"com.example.forum","skey":"self","config":{"policy":"public"}}"#,
        );
        roundtrip(
            &CreateSpaceOutput {
                uri: SPACE.to_string(),
            },
            r#"{"uri":"at://did:plc:auth/space/com.example.forum/self"}"#,
        );
    }

    #[test]
    fn space_management_inputs() {
        roundtrip(
            &UpdateSpaceInput {
                space: SPACE.to_string(),
                config: Config {
                    policy: Some("member-list".to_string()),
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
                    member_rev: "3jzfcijpj2z2a".to_string(),
                    added_at: "2026-01-01T00:00:00.000Z".to_string(),
                }],
            },
            r#"{"cursor":"c1","members":[{"did":"did:plc:member","memberRev":"3jzfcijpj2z2a","addedAt":"2026-01-01T00:00:00.000Z"}]}"#,
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
