//! Space-host role helpers: building the rsky-space-host authorization
//! machinery (policy, app access, jti store) from a local account's
//! `space_def` row. A PDS answers as space host only for spaces anchored on
//! its own accounts.

use crate::actor_store::space::{SpaceDefRow, SpaceStore};
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::space_auth::now_secs;
use anyhow::Result;
use async_trait::async_trait;
use rocket::State;
use rsky_lexicon::com::atproto::simplespace::{
    AppAccess as LexAppAccess, AppAccessAllowList, AppAccessOpen, Config as SimplespaceConfig,
    Policy as LexPolicy,
};
use rsky_space::space_id::SpaceId;
use rsky_space_host::appaccess::AppAccess;
use rsky_space_host::attestation::JtiStore;
use rsky_space_host::error::HostError;
use rsky_space_host::keys::ResolverDocSource;
use rsky_space_host::managing_app::HttpManagingApp;
use rsky_space_host::membership::InMemoryMembership;
use rsky_space_host::policy::Policy;
use rsky_space_host::signing::Signer;
use rsky_space_host::KeyResolver;
use secp256k1::Keypair;
use std::sync::Arc;

pub const POLICY_PUBLIC: &str = "public";
pub const POLICY_MEMBER_LIST: &str = "member-list";
pub const POLICY_MANAGING_APP: &str = "managing-app";
pub const APP_ACCESS_OPEN: &str = "open";
pub const APP_ACCESS_ALLOW_LIST: &str = "allow-list";

/// Ensure the space's authority is an account hosted here and return its
/// space definition. Only then can this PDS answer host methods for it.
pub async fn local_space_def(
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<crate::actor_store::blobstore::BlobstoreFactory>,
    space: &SpaceId,
) -> Result<(SpaceDefRow, SpaceStore, Keypair), ApiError> {
    if !actor_store.exists(&space.authority).await.unwrap_or(false) {
        return Err(ApiError::BadRequest(
            "SpaceNotFound".to_string(),
            format!("this host does not answer for {}", space.authority),
        ));
    }
    let reader = actor_store
        .read(
            space.authority.clone(),
            blobstore_factory.blobstore(space.authority.clone()),
        )
        .await
        .map_err(|error| ApiError::BadRequest("SpaceNotFound".to_string(), error.to_string()))?;
    let def = reader
        .space
        .live_space_def(&space.uri())
        .await
        .map_err(super::space_error)?;
    let keypair = reader.keypair().await.map_err(|error| {
        tracing::error!("missing authority keypair: {error}");
        ApiError::RuntimeError
    })?;
    Ok((def, reader.space.clone(), keypair))
}

/// The `getSpace` config surface for a definition row.
pub fn def_to_config(def: &SpaceDefRow) -> SimplespaceConfig {
    let policy = match def.policy.as_str() {
        POLICY_PUBLIC => LexPolicy::Public,
        POLICY_MANAGING_APP => LexPolicy::ManagingApp,
        _ => LexPolicy::MemberList,
    };
    let app_access = if def.app_access == APP_ACCESS_ALLOW_LIST {
        LexAppAccess::AllowList(AppAccessAllowList {
            allowed: def.allowed_clients.clone().unwrap_or_default(),
        })
    } else {
        LexAppAccess::Open(AppAccessOpen {})
    };
    SimplespaceConfig {
        policy: Some(policy),
        app_access: Some(app_access),
        managing_app: def.managing_app.clone(),
    }
}

pub fn def_app_access(def: &SpaceDefRow) -> AppAccess {
    if def.app_access == APP_ACCESS_ALLOW_LIST {
        AppAccess::AllowList(def.allowed_clients.clone().unwrap_or_default())
    } else {
        AppAccess::Open
    }
}

/// Build the per-user policy for a definition. `member-list` loads the member
/// rows; `managing-app` defers to the configured app over HTTP with authority
/// service auth.
pub async fn def_policy(
    def: &SpaceDefRow,
    space_store: &SpaceStore,
    signer: Signer,
    authority: &str,
    plc_url: &str,
) -> Result<Policy, ApiError> {
    match def.policy.as_str() {
        POLICY_PUBLIC => Ok(Policy::Public),
        POLICY_MANAGING_APP => {
            let service_id = def.managing_app.clone().ok_or_else(|| {
                ApiError::BadRequest(
                    "InvalidSpaceConfig".to_string(),
                    "managing-app policy without a managingApp".to_string(),
                )
            })?;
            let resolver = rsky_identity::did::did_resolver::DidResolver::new(
                rsky_identity::types::DidResolverOpts {
                    timeout: None,
                    plc_url: Some(plc_url.to_string()),
                    did_cache: Arc::new(rsky_identity::types::MemoryCache::new(None, None)),
                },
            );
            Ok(Policy::ManagingApp {
                service_id: service_id.clone(),
                client: Arc::new(HttpManagingApp::new(
                    service_id,
                    authority.to_string(),
                    signer,
                    Arc::new(ResolverDocSource::new(resolver)),
                    Arc::new(now_secs),
                    Arc::new(rsky_common::get_random_str),
                )),
            })
        }
        _ => {
            let mut members: Vec<String> = Vec::new();
            let mut cursor: Option<String> = None;
            loop {
                let page = space_store
                    .list_members(&def.space_uri, MEMBER_PAGE_SIZE, cursor.clone())
                    .await
                    .map_err(super::space_error)?;
                let Some(last) = page.last().cloned() else {
                    break;
                };
                let full_page = page.len() == MEMBER_PAGE_SIZE;
                members.extend(page);
                if !full_page {
                    break;
                }
                cursor = Some(last);
            }
            Ok(Policy::MemberList(Arc::new(InMemoryMembership::new(
                members,
            ))))
        }
    }
}

#[cfg(not(test))]
const MEMBER_PAGE_SIZE: usize = 1000;
#[cfg(test)]
const MEMBER_PAGE_SIZE: usize = 2;

/// Single-use nonce store over the authority's `space_used_jti` table.
pub struct ActorJtiStore(pub SpaceStore);

#[async_trait]
impl JtiStore for ActorJtiStore {
    async fn consume(&self, jti: &str, exp: u64) -> rsky_space_host::Result<bool> {
        self.0
            .consume_jti(jti, exp as i64, now_secs() as i64)
            .await
            .map_err(|e| HostError::Attestation(e.to_string()))
    }
}

/// The delegation-token issuer's signing key, resolved ahead of the mint call.
pub struct FixedKeyResolver(pub String);

#[async_trait]
impl KeyResolver for FixedKeyResolver {
    async fn signing_key(&self, _did: &str) -> rsky_space_host::Result<String> {
        Ok(self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor_store::db::get_migrated_db;

    const SPACE_URI: &str = "at://did:plc:auth/space/com.example.forum/main";

    fn def(policy: &str, app_access: &str) -> SpaceDefRow {
        SpaceDefRow {
            space_uri: SPACE_URI.to_string(),
            space_type: "com.example.forum".to_string(),
            skey: "main".to_string(),
            policy: policy.to_string(),
            app_access: app_access.to_string(),
            allowed_clients: None,
            managing_app: None,
            deleted: false,
        }
    }

    async fn store() -> (tempfile::TempDir, SpaceStore) {
        let dir = tempfile::tempdir().unwrap();
        let db = get_migrated_db(dir.path().join("store.sqlite"))
            .await
            .unwrap();
        (dir, SpaceStore::new("did:plc:auth".to_string(), db))
    }

    fn signer() -> Signer {
        Signer::from_secret(secp256k1::SecretKey::from_slice(&[0x66u8; 32]).unwrap())
    }

    #[test]
    fn def_to_config_covers_every_variant() {
        use rsky_lexicon::com::atproto::simplespace::Policy as LexPolicy;
        let cfg = def_to_config(&def(POLICY_PUBLIC, APP_ACCESS_OPEN));
        assert_eq!(cfg.policy, Some(LexPolicy::Public));
        assert!(matches!(cfg.app_access, Some(LexAppAccess::Open(_))));

        let mut allow = def(POLICY_MANAGING_APP, APP_ACCESS_ALLOW_LIST);
        allow.allowed_clients = Some(vec!["https://app.example/client".to_string()]);
        allow.managing_app = Some("did:web:app.example#managing_app".to_string());
        let cfg = def_to_config(&allow);
        assert_eq!(cfg.policy, Some(LexPolicy::ManagingApp));
        assert!(
            matches!(cfg.app_access, Some(LexAppAccess::AllowList(list)) if list.allowed.len() == 1)
        );
        assert_eq!(
            cfg.managing_app.as_deref(),
            Some("did:web:app.example#managing_app")
        );

        let cfg = def_to_config(&def(POLICY_MEMBER_LIST, APP_ACCESS_OPEN));
        assert_eq!(cfg.policy, Some(LexPolicy::MemberList));
    }

    #[test]
    fn def_app_access_maps_both_variants() {
        assert!(matches!(
            def_app_access(&def(POLICY_PUBLIC, APP_ACCESS_OPEN)),
            AppAccess::Open
        ));
        let mut allow = def(POLICY_PUBLIC, APP_ACCESS_ALLOW_LIST);
        allow.allowed_clients = Some(vec!["https://a".to_string()]);
        assert!(matches!(
            def_app_access(&allow),
            AppAccess::AllowList(list) if list == vec!["https://a".to_string()]
        ));
        // allow-list with no clients defaults to an empty (deny-all) list
        assert!(matches!(
            def_app_access(&def(POLICY_PUBLIC, APP_ACCESS_ALLOW_LIST)),
            AppAccess::AllowList(list) if list.is_empty()
        ));
    }

    #[tokio::test]
    async fn def_policy_public_and_member_list() {
        let (_dir, store) = store().await;
        let policy = def_policy(
            &def(POLICY_PUBLIC, APP_ACCESS_OPEN),
            &store,
            signer(),
            "did:plc:auth",
            "http://127.0.0.1:1",
        )
        .await
        .unwrap();
        assert!(policy
            .authorizes(SPACE_URI, "did:plc:anyone", None)
            .await
            .unwrap());

        // an empty member list denies everyone
        let policy = def_policy(
            &def(POLICY_MEMBER_LIST, APP_ACCESS_OPEN),
            &store,
            signer(),
            "did:plc:auth",
            "http://127.0.0.1:1",
        )
        .await
        .unwrap();
        assert!(!policy
            .authorizes(SPACE_URI, "did:plc:anyone", None)
            .await
            .unwrap());

        // member-list paginates through the full member set
        for i in 0..5 {
            store
                .add_member(SPACE_URI, &format!("did:plc:m{i}"))
                .await
                .unwrap();
        }
        let policy = def_policy(
            &def(POLICY_MEMBER_LIST, APP_ACCESS_OPEN),
            &store,
            signer(),
            "did:plc:auth",
            "http://127.0.0.1:1",
        )
        .await
        .unwrap();
        for i in 0..5 {
            assert!(policy
                .authorizes(SPACE_URI, &format!("did:plc:m{i}"), None)
                .await
                .unwrap());
        }
        assert!(!policy
            .authorizes(SPACE_URI, "did:plc:stranger", None)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn def_policy_managing_app() {
        let (_dir, store) = store().await;
        // Missing managingApp is a config error.
        let result = def_policy(
            &def(POLICY_MANAGING_APP, APP_ACCESS_OPEN),
            &store,
            signer(),
            "did:plc:auth",
            "http://127.0.0.1:1",
        )
        .await;
        let Err(err) = result else {
            panic!("managing-app without a managingApp must be a config error");
        };
        assert!(matches!(err, ApiError::BadRequest(name, _) if name == "InvalidSpaceConfig"));

        let mut with_app = def(POLICY_MANAGING_APP, APP_ACCESS_OPEN);
        with_app.managing_app = Some("did:web:app.example#managing_app".to_string());
        let policy = def_policy(
            &with_app,
            &store,
            signer(),
            "did:plc:auth",
            "http://127.0.0.1:1",
        )
        .await
        .unwrap();
        assert_eq!(
            policy.managing_app(),
            Some("did:web:app.example#managing_app")
        );
        // The app is unreachable in tests: the decision errors rather than allows.
        assert!(policy
            .authorizes(SPACE_URI, "did:plc:member", None)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn jti_store_and_key_resolver_adapters() {
        let (_dir, store) = store().await;
        let jti_store = ActorJtiStore(store);
        let exp = crate::space_auth::now_secs() + 60;
        assert!(jti_store.consume("nonce-1", exp).await.unwrap());
        assert!(!jti_store.consume("nonce-1", exp).await.unwrap());

        let resolver = FixedKeyResolver("did:key:zExample".to_string());
        assert_eq!(
            resolver.signing_key("did:plc:whoever").await.unwrap(),
            "did:key:zExample"
        );
    }
}
