//! Translation utilities between old simplespace config format and new policy format.

use crate::actor_store::space::SpacePolicy;
use crate::apis::ApiError;
#[allow(deprecated)]
use rsky_lexicon::com::atproto::simplespace;
use rsky_lexicon::com::atproto::{managedspace, memberspace};

/// Translate old simplespace config to new policy union.
///
/// Mapping:
/// - policy="public" -> memberspace with publicRead=true, publicWrite=true
/// - policy="member-list" -> memberspace with publicRead=false, publicWrite=false
/// - policy="managing-app" -> managedspace with managingApp
pub fn config_to_policy(config: &simplespace::Config) -> Result<SpacePolicy, ApiError> {
    let policy_type = config.policy.as_deref().unwrap_or("member-list");

    match policy_type {
        "public" => {
            let allowed_apps = extract_allowed_apps(&config.app_access);
            Ok(SpacePolicy::Memberspace(memberspace::Policy {
                public_read: Some(true),
                public_write: Some(true),
                allowed_apps,
                publish_policy: Some(false), // Old API didn't publish
                publish_members: Some(false),
            }))
        }
        "member-list" => {
            let allowed_apps = extract_allowed_apps(&config.app_access);
            Ok(SpacePolicy::Memberspace(memberspace::Policy {
                public_read: Some(false),
                public_write: Some(false),
                allowed_apps,
                publish_policy: Some(false),
                publish_members: Some(false),
            }))
        }
        "managing-app" => {
            let managing_app = config.managing_app.clone().ok_or_else(|| {
                tracing::warn!(
                    policy = %policy_type,
                    "Translation failed: managingApp required for managing-app policy"
                );
                ApiError::InvalidRequest(
                    "managingApp field is required when policy is 'managing-app'".to_string(),
                )
            })?;
            Ok(SpacePolicy::Managedspace(managedspace::Policy {
                managing_app,
                publish_policy: Some(false),
            }))
        }
        other => {
            tracing::warn!(
                policy = %other,
                "Translation failed: unknown policy type"
            );
            Err(ApiError::InvalidRequest(format!(
                "Unknown policy type '{}'. Valid values: public, member-list, managing-app",
                other
            )))
        }
    }
}

/// Translate new policy back to old config format (for getSpace response).
///
/// Note: Some information loss occurs (e.g., publishPolicy, publishMembers
/// flags don't exist in old format).
pub fn policy_to_config(policy: &SpacePolicy) -> simplespace::Config {
    match policy {
        SpacePolicy::Personalspace(_) => {
            // Personalspace didn't exist in old API - map to private member-list
            simplespace::Config {
                policy: Some("member-list".to_string()),
                app_access: Some(simplespace::AppAccess::Open(simplespace::AppAccessOpen {})),
                managing_app: None,
            }
        }
        SpacePolicy::Memberspace(policy) => {
            let is_public =
                policy.public_read.unwrap_or(false) && policy.public_write.unwrap_or(false);
            let app_access = match &policy.allowed_apps {
                Some(allowed) if !allowed.is_empty() => Some(simplespace::AppAccess::AllowList(
                    simplespace::AppAccessAllowList {
                        allowed: allowed.clone(),
                    },
                )),
                _ => Some(simplespace::AppAccess::Open(simplespace::AppAccessOpen {})),
            };

            simplespace::Config {
                policy: Some(if is_public {
                    "public".to_string()
                } else {
                    "member-list".to_string()
                }),
                app_access,
                managing_app: None,
            }
        }
        SpacePolicy::Managedspace(policy) => simplespace::Config {
            policy: Some("managing-app".to_string()),
            app_access: Some(simplespace::AppAccess::Open(simplespace::AppAccessOpen {})),
            managing_app: Some(policy.managing_app.clone()),
        },
    }
}

/// Extract allowed apps from old appAccess union.
fn extract_allowed_apps(app_access: &Option<simplespace::AppAccess>) -> Option<Vec<String>> {
    match app_access {
        Some(simplespace::AppAccess::AllowList(list)) => Some(list.allowed.clone()),
        _ => None,
    }
}

/// Translate old member (just DID) to new member (DID + read/write).
///
/// Old API granted full access to all members, so default to read=true, write=true.
pub fn old_member_to_new(did: String) -> memberspace::Member {
    memberspace::Member {
        did,
        read: true,
        write: true,
        member_rev: None,
        added_at: None,
    }
}

/// Translate new member to old member format.
///
/// The new memberspace::Member includes both new fields (read/write) and
/// legacy fields (member_rev/added_at). Old clients expect only the legacy fields.
///
/// IMPORTANT: The new Member type already includes member_rev and added_at as
/// optional fields (rsky extensions). The database stores these values, so they
/// will be populated when queried.
///
/// Information preserved: did, member_rev, added_at
/// Information discarded: read, write
pub fn new_member_to_old(member: &memberspace::Member) -> simplespace::Member {
    simplespace::Member {
        did: member.did.clone(),
        // Preserve legacy fields - these are already populated from database
        member_rev: member.member_rev.clone().unwrap_or_default(),
        added_at: member.added_at.clone().unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_public_config_to_policy() {
        let config = simplespace::Config {
            policy: Some("public".to_string()),
            app_access: Some(simplespace::AppAccess::Open(simplespace::AppAccessOpen {})),
            managing_app: None,
        };

        let policy = config_to_policy(&config).unwrap();
        assert!(matches!(policy, SpacePolicy::Memberspace(_)));

        if let SpacePolicy::Memberspace(p) = policy {
            assert_eq!(p.public_read, Some(true));
            assert_eq!(p.public_write, Some(true));
        }
    }

    #[test]
    fn test_member_list_config_to_policy() {
        let config = simplespace::Config {
            policy: Some("member-list".to_string()),
            app_access: Some(simplespace::AppAccess::AllowList(
                simplespace::AppAccessAllowList {
                    allowed: vec!["https://app.example.com".to_string()],
                },
            )),
            managing_app: None,
        };

        let policy = config_to_policy(&config).unwrap();

        if let SpacePolicy::Memberspace(p) = policy {
            assert_eq!(p.public_read, Some(false));
            assert_eq!(p.public_write, Some(false));
            assert_eq!(
                p.allowed_apps,
                Some(vec!["https://app.example.com".to_string()])
            );
        }
    }

    #[test]
    fn test_managing_app_config_to_policy() {
        let config = simplespace::Config {
            policy: Some("managing-app".to_string()),
            app_access: None,
            managing_app: Some("did:web:app.example.com#svc".to_string()),
        };

        let policy = config_to_policy(&config).unwrap();

        if let SpacePolicy::Managedspace(p) = policy {
            assert_eq!(p.managing_app, "did:web:app.example.com#svc");
        }
    }

    #[test]
    fn test_managing_app_missing_field_error() {
        let config = simplespace::Config {
            policy: Some("managing-app".to_string()),
            app_access: None,
            managing_app: None, // Missing!
        };

        let result = config_to_policy(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_policy_type_error() {
        let config = simplespace::Config {
            policy: Some("invalid-policy".to_string()),
            app_access: None,
            managing_app: None,
        };

        let result = config_to_policy(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_default_config_to_policy() {
        // When config is None/default, should default to member-list
        let config = simplespace::Config::default();

        let policy = config_to_policy(&config).unwrap();

        if let SpacePolicy::Memberspace(p) = policy {
            assert_eq!(p.public_read, Some(false));
            assert_eq!(p.public_write, Some(false));
        }
    }

    #[test]
    fn test_policy_to_config_public() {
        let policy = SpacePolicy::Memberspace(memberspace::Policy {
            public_read: Some(true),
            public_write: Some(true),
            allowed_apps: None,
            publish_policy: Some(false),
            publish_members: Some(false),
        });

        let config = policy_to_config(&policy);
        assert_eq!(config.policy, Some("public".to_string()));
    }

    #[test]
    fn test_policy_to_config_member_list() {
        let policy = SpacePolicy::Memberspace(memberspace::Policy {
            public_read: Some(false),
            public_write: Some(false),
            allowed_apps: Some(vec!["https://app.example.com".to_string()]),
            publish_policy: Some(false),
            publish_members: Some(false),
        });

        let config = policy_to_config(&policy);
        assert_eq!(config.policy, Some("member-list".to_string()));
        if let Some(simplespace::AppAccess::AllowList(list)) = config.app_access {
            assert_eq!(list.allowed, vec!["https://app.example.com".to_string()]);
        } else {
            panic!("Expected AllowList app_access");
        }
    }

    #[test]
    fn test_policy_to_config_managing_app() {
        let policy = SpacePolicy::Managedspace(managedspace::Policy {
            managing_app: "did:web:app.example.com#svc".to_string(),
            publish_policy: Some(false),
        });

        let config = policy_to_config(&policy);
        assert_eq!(config.policy, Some("managing-app".to_string()));
        assert_eq!(
            config.managing_app,
            Some("did:web:app.example.com#svc".to_string())
        );
    }

    #[test]
    fn test_policy_to_config_roundtrip() {
        let original_config = simplespace::Config {
            policy: Some("member-list".to_string()),
            app_access: Some(simplespace::AppAccess::Open(simplespace::AppAccessOpen {})),
            managing_app: None,
        };

        let policy = config_to_policy(&original_config).unwrap();
        let converted_config = policy_to_config(&policy);

        assert_eq!(converted_config.policy, Some("member-list".to_string()));
    }

    #[test]
    fn test_old_member_to_new() {
        let member = old_member_to_new("did:plc:example".to_string());
        assert_eq!(member.did, "did:plc:example");
        assert!(member.read);
        assert!(member.write);
    }

    #[test]
    fn test_new_member_to_old() {
        let new_member = memberspace::Member {
            did: "did:plc:example".to_string(),
            read: true,
            write: true,
            member_rev: Some("rev123".to_string()),
            added_at: Some("2026-01-01T00:00:00Z".to_string()),
        };

        let old_member = new_member_to_old(&new_member);
        assert_eq!(old_member.did, "did:plc:example");
        assert_eq!(old_member.member_rev, "rev123");
        assert_eq!(old_member.added_at, "2026-01-01T00:00:00Z");
    }

    #[test]
    fn test_new_member_to_old_with_empty_fields() {
        let new_member = memberspace::Member {
            did: "did:plc:example".to_string(),
            read: false,
            write: true,
            member_rev: None,
            added_at: None,
        };

        let old_member = new_member_to_old(&new_member);
        assert_eq!(old_member.did, "did:plc:example");
        assert_eq!(old_member.member_rev, ""); // Empty string for None
        assert_eq!(old_member.added_at, ""); // Empty string for None
    }
}
