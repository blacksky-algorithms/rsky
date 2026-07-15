//! The per-user access policy (spec §Configuration).
//!
//! `Policy` decides whether to authorize a *user*; the per-app axis is
//! [`crate::appaccess::AppAccess`]. Both must pass for a credential to be
//! minted.

use rsky_lexicon::com::atproto::simplespace::Policy as LexPolicy;
use std::sync::Arc;

use crate::error::Result;
use crate::managing_app::ManagingAppClient;
use crate::membership::MembershipStore;

/// How the authority decides whether to authorize a user at mint time.
pub enum Policy {
    /// Authorize any requester.
    Public,
    /// Authorize requesters present on the member list.
    MemberList(Arc<dyn MembershipStore>),
    /// Defer to the space's managing app via `checkUserAccess`; the host never
    /// reads the membership decision directly.
    ManagingApp {
        service_id: String,
        client: Arc<dyn ManagingAppClient>,
    },
}

impl Policy {
    pub async fn authorizes(
        &self,
        space_uri: &str,
        user_did: &str,
        attested_client_id: Option<&str>,
    ) -> Result<bool> {
        match self {
            Policy::Public => Ok(true),
            Policy::MemberList(members) => members.is_member(user_did).await,
            Policy::ManagingApp { client, .. } => {
                client
                    .check_user_access(space_uri, user_did, attested_client_id)
                    .await
            }
        }
    }

    /// The lexicon `policy` value surfaced by `getSpace`.
    pub fn lexicon_policy(&self) -> LexPolicy {
        match self {
            Policy::Public => LexPolicy::Public,
            Policy::MemberList(_) => LexPolicy::MemberList,
            Policy::ManagingApp { .. } => LexPolicy::ManagingApp,
        }
    }

    /// The managing app's service identifier, when this policy defers to one.
    pub fn managing_app(&self) -> Option<&str> {
        match self {
            Policy::ManagingApp { service_id, .. } => Some(service_id),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::HostError;
    use crate::membership::InMemoryMembership;
    use async_trait::async_trait;

    const SPACE: &str = "at://did:plc:auth/space/community.blacksky.feed/main";

    struct RecordingApp {
        allow: bool,
        seen: std::sync::Mutex<Vec<(String, String, Option<String>)>>,
    }

    #[async_trait]
    impl ManagingAppClient for RecordingApp {
        async fn check_user_access(
            &self,
            space: &str,
            user_did: &str,
            client_id: Option<&str>,
        ) -> Result<bool> {
            self.seen.lock().unwrap().push((
                space.to_string(),
                user_did.to_string(),
                client_id.map(str::to_string),
            ));
            Ok(self.allow)
        }
    }

    struct FailingApp;
    #[async_trait]
    impl ManagingAppClient for FailingApp {
        async fn check_user_access(&self, _: &str, _: &str, _: Option<&str>) -> Result<bool> {
            Err(HostError::ManagingApp("unreachable".into()))
        }
    }

    #[tokio::test]
    async fn public_authorizes_anyone() {
        let policy = Policy::Public;
        assert!(policy
            .authorizes(SPACE, "did:plc:anyone", None)
            .await
            .unwrap());
        assert_eq!(policy.lexicon_policy(), LexPolicy::Public);
        assert!(policy.managing_app().is_none());
    }

    #[tokio::test]
    async fn member_list_consults_the_store() {
        let members = InMemoryMembership::new(["did:plc:member".to_string()]);
        let policy = Policy::MemberList(Arc::new(members));
        assert!(policy
            .authorizes(SPACE, "did:plc:member", None)
            .await
            .unwrap());
        assert!(!policy
            .authorizes(SPACE, "did:plc:stranger", None)
            .await
            .unwrap());
        assert_eq!(policy.lexicon_policy(), LexPolicy::MemberList);
        assert!(policy.managing_app().is_none());
    }

    #[tokio::test]
    async fn managing_app_routes_the_decision_to_the_app() {
        let app = Arc::new(RecordingApp {
            allow: true,
            seen: std::sync::Mutex::new(vec![]),
        });
        let policy = Policy::ManagingApp {
            service_id: "did:web:app.example#managing_app".to_string(),
            client: app.clone(),
        };
        assert!(policy
            .authorizes(SPACE, "did:plc:member", Some("https://client.example"))
            .await
            .unwrap());
        let seen = app.seen.lock().unwrap();
        assert_eq!(
            *seen,
            vec![(
                SPACE.to_string(),
                "did:plc:member".to_string(),
                Some("https://client.example".to_string())
            )]
        );
        assert_eq!(policy.lexicon_policy(), LexPolicy::ManagingApp);
        assert_eq!(
            policy.managing_app(),
            Some("did:web:app.example#managing_app")
        );
    }

    #[tokio::test]
    async fn managing_app_denial_and_failure_propagate() {
        let deny = Policy::ManagingApp {
            service_id: "did:web:app.example#managing_app".to_string(),
            client: Arc::new(RecordingApp {
                allow: false,
                seen: std::sync::Mutex::new(vec![]),
            }),
        };
        assert!(!deny
            .authorizes(SPACE, "did:plc:member", None)
            .await
            .unwrap());

        let failing = Policy::ManagingApp {
            service_id: "did:web:app.example#managing_app".to_string(),
            client: Arc::new(FailingApp),
        };
        assert!(matches!(
            failing.authorizes(SPACE, "did:plc:member", None).await,
            Err(HostError::ManagingApp(_))
        ));
    }
}
