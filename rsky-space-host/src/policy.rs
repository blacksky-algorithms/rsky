//! The space's policy object (spec §Space policy).
//!
//! `Policy` decides whether to authorize a *user* for an [`Access`] kind;
//! `memberspace.allowedApps` additionally gates the app axis at credential
//! mint. Reading and writing are independent permissions: read access
//! controls whether a space credential is minted, write access controls
//! whether the authority tracks the writer and forwards `notifyWrite`.

pub use rsky_lexicon::com::atproto::managedspace::Access;
use rsky_lexicon::com::atproto::{
    managedspace as lex_managedspace, memberspace as lex_memberspace,
    personalspace as lex_personalspace, space as lex_space,
};
use std::sync::Arc;

use crate::error::Result;
use crate::managing_app::ManagingAppClient;
use crate::membership::MembershipStore;

/// The space's policy: which users are authorized, and for what.
pub enum Policy {
    /// Only the space authority's own account has access.
    Personalspace,
    /// Access checked against a member list maintained by the PDS.
    Memberspace {
        public_read: bool,
        public_write: bool,
        /// Apps the authority authorizes for reads; any app when `None`.
        allowed_apps: Option<Vec<String>>,
        publish_policy: bool,
        publish_members: bool,
        members: Arc<dyn MembershipStore>,
    },
    /// Each user access decision deferred to the space's managing app via
    /// `com.atproto.managedspace.checkUserAccess`; the host never reads the
    /// decision directly.
    Managedspace {
        service_id: String,
        publish_policy: bool,
        client: Arc<dyn ManagingAppClient>,
    },
}

impl Policy {
    /// Whether `user_did` (absent for an anonymous request) is authorized for
    /// `access`. The space authority is always authorized for both kinds.
    pub async fn authorizes(
        &self,
        space_uri: &str,
        user_did: Option<&str>,
        access: Access,
        attested_client_id: Option<&str>,
    ) -> Result<bool> {
        // The authority is implicitly authorized for their own space under
        // every policy, for both reading and writing, without appearing on a
        // member list or being approved by a managing app.
        if let (Ok(space), Some(did)) = (rsky_space::space_id::SpaceId::parse(space_uri), user_did)
        {
            if space.authority == did {
                return Ok(true);
            }
        }
        match self {
            Policy::Personalspace => Ok(false),
            Policy::Memberspace {
                public_read,
                public_write,
                members,
                ..
            } => {
                if match access {
                    Access::Read => *public_read,
                    Access::Write => *public_write,
                } {
                    return Ok(true);
                }
                let Some(did) = user_did else {
                    return Ok(false);
                };
                let member = members.member_access(did).await?;
                Ok(match access {
                    Access::Read => member.read,
                    Access::Write => member.write,
                })
            }
            Policy::Managedspace { client, .. } => {
                let Some(did) = user_did else {
                    return Ok(false);
                };
                client
                    .check_user_access(space_uri, did, access, attested_client_id)
                    .await
            }
        }
    }

    /// Whether a client attestation is required at credential mint: only a
    /// memberspace with `allowedApps` gates on app identity.
    pub fn requires_attestation(&self) -> bool {
        matches!(
            self,
            Policy::Memberspace {
                allowed_apps: Some(_),
                ..
            }
        )
    }

    /// Whether the attested `client_id` (if any) is permitted to read. Does
    /// not apply to writes: write notifications authenticate the writer's
    /// repo host, not the client application.
    pub fn permits_client(&self, attested_client_id: Option<&str>) -> bool {
        match self {
            Policy::Memberspace {
                allowed_apps: Some(allowed),
                ..
            } => attested_client_id
                .map(|c| allowed.iter().any(|a| a == c))
                .unwrap_or(false),
            _ => true,
        }
    }

    /// The policy object surfaced by `getSpace`.
    pub fn lexicon_policy(&self) -> lex_space::Policy {
        match self {
            Policy::Personalspace => lex_space::Policy::Personalspace(lex_personalspace::Policy {}),
            Policy::Memberspace {
                public_read,
                public_write,
                allowed_apps,
                publish_policy,
                publish_members,
                ..
            } => lex_space::Policy::Memberspace(lex_memberspace::Policy {
                public_read: Some(*public_read),
                public_write: Some(*public_write),
                allowed_apps: allowed_apps.clone(),
                publish_policy: Some(*publish_policy),
                publish_members: Some(*publish_members),
            }),
            Policy::Managedspace {
                service_id,
                publish_policy,
                ..
            } => lex_space::Policy::Managedspace(lex_managedspace::Policy {
                managing_app: service_id.clone(),
                publish_policy: Some(*publish_policy),
            }),
        }
    }

    /// The managing app's service identifier, when this policy defers to one.
    pub fn managing_app(&self) -> Option<&str> {
        match self {
            Policy::Managedspace { service_id, .. } => Some(service_id),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::HostError;
    use crate::membership::{InMemoryMembership, MemberAccess};
    use async_trait::async_trait;

    const SPACE: &str = "at://did:plc:auth/space/community.blacksky.feed/main";

    struct RecordingApp {
        allow: bool,
        seen: std::sync::Mutex<Vec<(String, String, Access, Option<String>)>>,
    }

    #[async_trait]
    impl ManagingAppClient for RecordingApp {
        async fn check_user_access(
            &self,
            space: &str,
            user_did: &str,
            access: Access,
            client_id: Option<&str>,
        ) -> Result<bool> {
            self.seen.lock().unwrap().push((
                space.to_string(),
                user_did.to_string(),
                access,
                client_id.map(str::to_string),
            ));
            Ok(self.allow)
        }
    }

    struct FailingApp;
    #[async_trait]
    impl ManagingAppClient for FailingApp {
        async fn check_user_access(
            &self,
            _: &str,
            _: &str,
            _: Access,
            _: Option<&str>,
        ) -> Result<bool> {
            Err(HostError::ManagingApp("unreachable".into()))
        }
    }

    fn memberspace(public_read: bool, public_write: bool) -> Policy {
        Policy::Memberspace {
            public_read,
            public_write,
            allowed_apps: None,
            publish_policy: false,
            publish_members: false,
            members: Arc::new(InMemoryMembership::default()),
        }
    }

    #[tokio::test]
    async fn authority_is_always_authorized() {
        for policy in [
            Policy::Personalspace,
            memberspace(false, false),
            Policy::Managedspace {
                service_id: "did:web:app#svc".to_string(),
                publish_policy: false,
                client: Arc::new(FailingApp),
            },
        ] {
            assert!(policy
                .authorizes(SPACE, Some("did:plc:auth"), Access::Read, None)
                .await
                .unwrap());
            assert!(policy
                .authorizes(SPACE, Some("did:plc:auth"), Access::Write, None)
                .await
                .unwrap());
        }
    }

    #[tokio::test]
    async fn personalspace_denies_everyone_else() {
        let policy = Policy::Personalspace;
        assert!(!policy
            .authorizes(SPACE, Some("did:plc:anyone"), Access::Read, None)
            .await
            .unwrap());
        assert!(!policy
            .authorizes(SPACE, Some("did:plc:anyone"), Access::Write, None)
            .await
            .unwrap());
        assert!(!policy
            .authorizes(SPACE, None, Access::Read, None)
            .await
            .unwrap());
        assert_eq!(
            policy.lexicon_policy().type_name(),
            "com.atproto.personalspace.policy"
        );
        assert!(policy.managing_app().is_none());
    }

    #[tokio::test]
    async fn memberspace_consults_member_access_per_kind() {
        let members = InMemoryMembership::with_access([
            (
                "did:plc:reader".to_string(),
                MemberAccess {
                    read: true,
                    write: false,
                },
            ),
            (
                "did:plc:writer".to_string(),
                MemberAccess {
                    read: false,
                    write: true,
                },
            ),
        ]);
        let policy = Policy::Memberspace {
            public_read: false,
            public_write: false,
            allowed_apps: None,
            publish_policy: false,
            publish_members: false,
            members: Arc::new(members),
        };
        assert!(policy
            .authorizes(SPACE, Some("did:plc:reader"), Access::Read, None)
            .await
            .unwrap());
        assert!(!policy
            .authorizes(SPACE, Some("did:plc:reader"), Access::Write, None)
            .await
            .unwrap());
        assert!(!policy
            .authorizes(SPACE, Some("did:plc:writer"), Access::Read, None)
            .await
            .unwrap());
        assert!(policy
            .authorizes(SPACE, Some("did:plc:writer"), Access::Write, None)
            .await
            .unwrap());
        assert!(!policy
            .authorizes(SPACE, Some("did:plc:stranger"), Access::Read, None)
            .await
            .unwrap());
        // Anonymous: denied unless a public flag covers the access kind.
        assert!(!policy
            .authorizes(SPACE, None, Access::Read, None)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn memberspace_public_flags_cover_their_access_kind() {
        let policy = memberspace(true, false);
        assert!(policy
            .authorizes(SPACE, Some("did:plc:anyone"), Access::Read, None)
            .await
            .unwrap());
        assert!(policy
            .authorizes(SPACE, None, Access::Read, None)
            .await
            .unwrap());
        assert!(!policy
            .authorizes(SPACE, Some("did:plc:anyone"), Access::Write, None)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn memberspace_allowed_apps_gates_the_client_axis() {
        let Policy::Memberspace { members, .. } = memberspace(false, false) else {
            unreachable!()
        };
        let policy = Policy::Memberspace {
            public_read: false,
            public_write: false,
            allowed_apps: Some(vec!["https://app.example/client".to_string()]),
            publish_policy: false,
            publish_members: false,
            members,
        };
        assert!(policy.requires_attestation());
        assert!(!policy.permits_client(None));
        assert!(!policy.permits_client(Some("https://other.example/client")));
        assert!(policy.permits_client(Some("https://app.example/client")));

        let open = memberspace(false, false);
        assert!(!open.requires_attestation());
        assert!(open.permits_client(None));
        assert!(open.permits_client(Some("https://anything.example")));
    }

    #[tokio::test]
    async fn managedspace_routes_the_decision_to_the_app() {
        let app = Arc::new(RecordingApp {
            allow: true,
            seen: std::sync::Mutex::new(vec![]),
        });
        let policy = Policy::Managedspace {
            service_id: "did:web:app.example#managing_app".to_string(),
            publish_policy: false,
            client: app.clone(),
        };
        assert!(policy
            .authorizes(
                SPACE,
                Some("did:plc:member"),
                Access::Write,
                Some("https://client.example")
            )
            .await
            .unwrap());
        let seen = app.seen.lock().unwrap();
        assert_eq!(
            *seen,
            vec![(
                SPACE.to_string(),
                "did:plc:member".to_string(),
                Access::Write,
                Some("https://client.example".to_string())
            )]
        );
        assert_eq!(
            policy.managing_app(),
            Some("did:web:app.example#managing_app")
        );
    }

    #[tokio::test]
    async fn managedspace_denial_and_failure_propagate() {
        let deny = Policy::Managedspace {
            service_id: "did:web:app#svc".to_string(),
            publish_policy: false,
            client: Arc::new(RecordingApp {
                allow: false,
                seen: std::sync::Mutex::new(vec![]),
            }),
        };
        assert!(!deny
            .authorizes(SPACE, Some("did:plc:member"), Access::Read, None)
            .await
            .unwrap());
        let failing = Policy::Managedspace {
            service_id: "did:web:app#svc".to_string(),
            publish_policy: false,
            client: Arc::new(FailingApp),
        };
        assert!(failing
            .authorizes(SPACE, Some("did:plc:member"), Access::Read, None)
            .await
            .is_err());
    }
}
