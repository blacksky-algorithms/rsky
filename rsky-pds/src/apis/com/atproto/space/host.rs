//! Space-host role helpers: building the rsky-space-host authorization
//! machinery (policy, app access, jti store) from a local account's
//! `space_def` row. A PDS answers as space host only for spaces anchored on
//! its own accounts.

use crate::actor_store::space::{SpaceDefRow, SpacePolicy, SpaceStore};
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::space_auth::now_secs;
use anyhow::Result;
use async_trait::async_trait;
use rocket::State;
use rsky_space::space_id::SpaceId;
use rsky_space_host::attestation::JtiStore;
use rsky_space_host::error::HostError;
use rsky_space_host::keys::ResolverDocSource;
use rsky_space_host::managing_app::HttpManagingApp;
use rsky_space_host::membership::{InMemoryMembership, MemberAccess};
use rsky_space_host::policy::Policy;
use rsky_space_host::signing::Signer;
use rsky_space_host::KeyResolver;
use secp256k1::Keypair;
use std::sync::Arc;

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
    let keypair = reader
        .keypair()
        .await
        .map_err(super::internal_error("missing authority keypair"))?;
    Ok((def, reader.space.clone(), keypair))
}

/// The policy object a definition row surfaces through `getSpace`.
pub fn def_to_policy(def: &SpaceDefRow) -> SpacePolicy {
    def.policy.clone()
}

/// Parse and validate a policy object from a `createSpace`/`updateSpace`
/// request body. An unrecognized `$type` or a malformed object is an
/// `UnsupportedPolicy` error per spec; a `managedspace` additionally requires
/// a well-formed `managingApp` service identifier.
pub fn parse_policy(value: serde_json::Value) -> Result<SpacePolicy, ApiError> {
    let policy: SpacePolicy = serde_json::from_value(value).map_err(|error| {
        ApiError::BadRequest("UnsupportedPolicy".to_string(), error.to_string())
    })?;
    if let SpacePolicy::Managedspace(ref managed) = policy {
        if !managed.managing_app.starts_with("did:") || !managed.managing_app.contains('#') {
            return Err(ApiError::InvalidRequest(format!(
                "managingApp must be a service identifier (did#fragment): {}",
                managed.managing_app
            )));
        }
    }
    Ok(policy)
}

/// Build the runtime policy for a definition row. `memberspace` loads the
/// member rows with their read/write access; `managedspace` defers to the
/// configured app over HTTP with authority service auth.
pub async fn def_policy(
    def: &SpaceDefRow,
    space_store: &SpaceStore,
    signer: Signer,
    authority: &str,
    plc_url: &str,
) -> Result<Policy, ApiError> {
    match &def.policy {
        SpacePolicy::Personalspace(_) => Ok(Policy::Personalspace),
        SpacePolicy::Managedspace(managed) => {
            let resolver = rsky_identity::did::did_resolver::DidResolver::new(
                rsky_identity::types::DidResolverOpts {
                    timeout: None,
                    plc_url: Some(plc_url.to_string()),
                    did_cache: Arc::new(rsky_identity::types::MemoryCache::new(None, None)),
                },
            );
            Ok(Policy::Managedspace {
                service_id: managed.managing_app.clone(),
                publish_policy: managed.publish_policy.unwrap_or(false),
                client: Arc::new(HttpManagingApp::new(
                    managed.managing_app.clone(),
                    authority.to_string(),
                    signer,
                    Arc::new(ResolverDocSource::new(resolver)),
                    Arc::new(now_secs),
                    Arc::new(rsky_common::get_random_str),
                )),
            })
        }
        SpacePolicy::Memberspace(memberspace) => {
            let mut members: Vec<(String, MemberAccess)> = Vec::new();
            let mut cursor: Option<String> = None;
            loop {
                let page = space_store
                    .list_members(&def.space_uri, MEMBER_PAGE_SIZE, cursor.clone())
                    .await
                    .map_err(super::space_error)?;
                let Some(last) = page.last().map(|row| row.did.clone()) else {
                    break;
                };
                let full_page = page.len() == MEMBER_PAGE_SIZE;
                members.extend(page.into_iter().map(|row| {
                    (
                        row.did,
                        MemberAccess {
                            read: row.read,
                            write: row.write,
                        },
                    )
                }));
                if !full_page {
                    break;
                }
                cursor = Some(last);
            }
            Ok(Policy::Memberspace {
                public_read: memberspace.public_read.unwrap_or(false),
                public_write: memberspace.public_write.unwrap_or(false),
                allowed_apps: memberspace.allowed_apps.clone(),
                publish_policy: memberspace.publish_policy.unwrap_or(false),
                publish_members: memberspace.publish_members.unwrap_or(false),
                members: Arc::new(InMemoryMembership::with_access(members)),
            })
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
    use rsky_lexicon::com::atproto::managedspace::Access;
    use rsky_lexicon::com::atproto::{managedspace, memberspace, personalspace};

    const SPACE_URI: &str = "at://did:plc:auth/space/com.example.forum/main";

    fn def(policy: SpacePolicy) -> SpaceDefRow {
        SpaceDefRow {
            space_uri: SPACE_URI.to_string(),
            space_type: "com.example.forum".to_string(),
            skey: "main".to_string(),
            policy,
            deleted: false,
        }
    }

    fn memberspace_def() -> SpaceDefRow {
        def(SpacePolicy::Memberspace(memberspace::Policy::default()))
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
    fn parse_policy_maps_baseline_types() {
        assert!(matches!(
            parse_policy(serde_json::json!({"$type": "com.atproto.personalspace.policy"})).unwrap(),
            SpacePolicy::Personalspace(_)
        ));
        assert!(matches!(
            parse_policy(serde_json::json!({"$type": "com.atproto.memberspace.policy", "publicRead": true})).unwrap(),
            SpacePolicy::Memberspace(ref p) if p.public_read == Some(true)
        ));
        assert!(matches!(
            parse_policy(serde_json::json!({"$type": "com.atproto.managedspace.policy", "managingApp": "did:web:app#svc"})).unwrap(),
            SpacePolicy::Managedspace(ref p) if p.managing_app == "did:web:app#svc"
        ));
        // Unknown $type and malformed objects are UnsupportedPolicy.
        for bad in [
            serde_json::json!({"$type": "com.example.custom.policy"}),
            serde_json::json!({"$type": "com.atproto.managedspace.policy"}),
            serde_json::json!({"publicRead": true}),
        ] {
            let err = parse_policy(bad).unwrap_err();
            assert!(
                matches!(&err, ApiError::BadRequest(name, _) if name == "UnsupportedPolicy"),
                "{err:?}"
            );
        }
        // A managedspace service identifier must be did#fragment.
        assert!(parse_policy(serde_json::json!({
            "$type": "com.atproto.managedspace.policy",
            "managingApp": "not-a-service-id"
        }))
        .is_err());
    }

    #[test]
    fn def_to_policy_is_the_stored_object() {
        let policy = SpacePolicy::Personalspace(personalspace::Policy {});
        assert_eq!(def_to_policy(&def(policy.clone())), policy);
    }

    #[tokio::test]
    async fn def_policy_personalspace_and_memberspace() {
        let (_dir, store) = store().await;
        let policy = def_policy(
            &def(SpacePolicy::Personalspace(personalspace::Policy {})),
            &store,
            signer(),
            "did:plc:auth",
            "http://127.0.0.1:1",
        )
        .await
        .unwrap();
        assert!(!policy
            .authorizes(SPACE_URI, Some("did:plc:anyone"), Access::Read, None)
            .await
            .unwrap());

        // An empty member list denies non-members per kind; public flags open
        // the corresponding axis.
        let policy = def_policy(
            &memberspace_def(),
            &store,
            signer(),
            "did:plc:auth",
            "http://127.0.0.1:1",
        )
        .await
        .unwrap();
        assert!(!policy
            .authorizes(SPACE_URI, Some("did:plc:anyone"), Access::Read, None)
            .await
            .unwrap());

        // memberspace loads each member's read/write booleans
        store
            .put_member(SPACE_URI, "did:plc:reader", true, false)
            .await
            .unwrap();
        for i in 0..5 {
            store
                .put_member(SPACE_URI, &format!("did:plc:m{i}"), true, true)
                .await
                .unwrap();
        }
        let policy = def_policy(
            &memberspace_def(),
            &store,
            signer(),
            "did:plc:auth",
            "http://127.0.0.1:1",
        )
        .await
        .unwrap();
        assert!(policy
            .authorizes(SPACE_URI, Some("did:plc:reader"), Access::Read, None)
            .await
            .unwrap());
        assert!(!policy
            .authorizes(SPACE_URI, Some("did:plc:reader"), Access::Write, None)
            .await
            .unwrap());
        for i in 0..5 {
            assert!(policy
                .authorizes(
                    SPACE_URI,
                    Some(&format!("did:plc:m{i}")),
                    Access::Write,
                    None
                )
                .await
                .unwrap());
        }
        assert!(!policy
            .authorizes(SPACE_URI, Some("did:plc:stranger"), Access::Read, None)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn def_policy_managedspace() {
        let (_dir, store) = store().await;
        let with_app = def(SpacePolicy::Managedspace(managedspace::Policy {
            managing_app: "did:web:app.example#managing_app".to_string(),
            publish_policy: None,
        }));
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
            .authorizes(SPACE_URI, Some("did:plc:member"), Access::Read, None)
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
