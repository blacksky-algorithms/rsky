//! The space authority: mints space credentials (spec §Access control).

use async_trait::async_trait;
use rsky_lexicon::com::atproto::simplespace::{
    AppAccess as LexAppAccess, AppAccessAllowList, AppAccessOpen, Config as SimplespaceConfig,
};
use rsky_space::credential::{
    self, Confirmation, JwtHeader, SpaceClaims, CREDENTIAL_TTL_SECS, CREDENTIAL_TYP,
};
use rsky_space::space_id::SpaceId;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock};

use crate::appaccess::AppAccess;
use crate::attestation::{verify_client_attestation, JtiStore, MetadataFetcher};
use crate::error::{HostError, Result};
use crate::notify::Notifier;
use crate::policy::Policy;
use crate::registration::LifecycleAcker;
use crate::signing::Signer;

/// Everything derived from one space authority: the authority itself plus the
/// collaborators that mint, verify, or sign in its name.
pub struct AuthorityContext {
    pub authority: Arc<Authority>,
    pub policy: Arc<Policy>,
    pub notifier: Arc<dyn Notifier>,
    pub lifecycle_acker: Option<Arc<dyn LifecycleAcker>>,
}

impl AuthorityContext {
    pub fn authority_did(&self) -> &str {
        self.authority.authority_did()
    }
}

/// Builds the [`AuthorityContext`] for an authority first seen at
/// registration time; fails when the authority's signing key is unavailable.
pub type AuthorityFactory = Arc<dyn Fn(&SpaceId) -> Result<Arc<AuthorityContext>> + Send + Sync>;

/// The authorities this host answers for, keyed by authority DID.
#[derive(Default)]
pub struct AuthorityRegistry {
    contexts: RwLock<BTreeMap<String, Arc<AuthorityContext>>>,
}

impl AuthorityRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, context: Arc<AuthorityContext>) {
        self.contexts
            .write()
            .expect("authority registry")
            .insert(context.authority_did().to_string(), context);
    }

    /// Insert unless the authority is already present; returns the context
    /// that ends up registered either way.
    pub fn insert_if_absent(&self, context: Arc<AuthorityContext>) -> Arc<AuthorityContext> {
        self.contexts
            .write()
            .expect("authority registry")
            .entry(context.authority_did().to_string())
            .or_insert(context)
            .clone()
    }

    pub fn authority(&self, authority_did: &str) -> Result<Arc<AuthorityContext>> {
        self.contexts
            .read()
            .expect("authority registry")
            .get(authority_did)
            .cloned()
            .ok_or_else(|| HostError::SpaceNotFound(authority_did.to_string()))
    }

    pub fn for_space(&self, space_uri: &str) -> Result<Arc<AuthorityContext>> {
        let space = SpaceId::parse(space_uri)
            .map_err(|_| HostError::SpaceNotFound(space_uri.to_string()))?;
        self.authority(&space.authority)
            .map_err(|_| HostError::SpaceNotFound(space_uri.to_string()))
    }

    pub fn contexts(&self) -> Vec<Arc<AuthorityContext>> {
        self.contexts
            .read()
            .expect("authority registry")
            .values()
            .cloned()
            .collect()
    }
}

/// Resolves an account's atproto signing `did:key` (from its DID document), used
/// to verify a delegation token minted by that user's PDS.
#[async_trait]
pub trait KeyResolver: Send + Sync {
    async fn service_key(&self, did: &str) -> Result<String>;
}

/// A space authority for one or more spaces of a single type.
pub struct Authority {
    pub space: SpaceId,
    hosted: RwLock<BTreeSet<String>>,
    registered: RwLock<BTreeSet<String>>,
    pub signer: Signer,
    pub app_access: AppAccess,
}

impl Authority {
    pub fn new(space: SpaceId, signer: Signer, app_access: AppAccess) -> Self {
        Self::new_many(
            space.authority.clone(),
            space.space_type.clone(),
            [space.skey.clone()],
            signer,
            app_access,
        )
    }

    pub fn new_many(
        authority_did: impl Into<String>,
        space_type: impl Into<String>,
        skeys: impl IntoIterator<Item = String>,
        signer: Signer,
        app_access: AppAccess,
    ) -> Self {
        let authority_did = authority_did.into();
        let space_type = space_type.into();
        let hosted: BTreeSet<String> = skeys.into_iter().collect();
        let first_skey = hosted
            .first()
            .expect("space authority needs at least one hosted space")
            .clone();
        Self {
            space: SpaceId::new(authority_did, space_type, first_skey),
            registered: RwLock::new(hosted.clone()),
            hosted: RwLock::new(hosted),
            signer,
            app_access,
        }
    }

    pub fn authority_did(&self) -> &str {
        &self.space.authority
    }

    pub fn space_uri(&self) -> String {
        self.space.uri()
    }

    pub fn space(&self, skey: &str) -> SpaceId {
        SpaceId::new(&self.space.authority, &self.space.space_type, skey)
    }

    pub fn spaces(&self) -> Vec<String> {
        self.hosted
            .read()
            .expect("hosted spaces")
            .iter()
            .map(|skey| self.space(skey).uri())
            .collect()
    }

    pub fn resolve(&self, space_uri: &str) -> Result<SpaceId> {
        let space = SpaceId::parse(space_uri)
            .map_err(|_| HostError::SpaceNotFound(space_uri.to_string()))?;
        if space.authority == self.space.authority
            && space.space_type == self.space.space_type
            && self
                .hosted
                .read()
                .expect("hosted spaces")
                .contains(&space.skey)
        {
            Ok(space)
        } else {
            Err(HostError::SpaceNotFound(space_uri.to_string()))
        }
    }

    pub fn register(&self, space_uri: &str) -> Result<bool> {
        let space = SpaceId::parse(space_uri)
            .map_err(|_| HostError::SpaceNotFound(space_uri.to_string()))?;
        if space.authority != self.space.authority || space.space_type != self.space.space_type {
            return Err(HostError::SpaceNotFound(space_uri.to_string()));
        }
        let newly_registered = self
            .registered
            .write()
            .expect("registered spaces")
            .insert(space.skey.clone());
        self.hosted
            .write()
            .expect("hosted spaces")
            .insert(space.skey);
        Ok(newly_registered)
    }

    pub fn resolve_registered(&self, space_uri: &str) -> Result<SpaceId> {
        let space = SpaceId::parse(space_uri)
            .map_err(|_| HostError::SpaceNotFound(space_uri.to_string()))?;
        if space.authority == self.space.authority
            && space.space_type == self.space.space_type
            && self
                .registered
                .read()
                .expect("registered spaces")
                .contains(&space.skey)
        {
            Ok(space)
        } else {
            Err(HostError::SpaceNotFound(space_uri.to_string()))
        }
    }

    pub fn unregister(&self, space_uri: &str) -> Result<bool> {
        let space = SpaceId::parse(space_uri)
            .map_err(|_| HostError::SpaceNotFound(space_uri.to_string()))?;
        if space.authority != self.space.authority || space.space_type != self.space.space_type {
            return Err(HostError::SpaceNotFound(space_uri.to_string()));
        }
        let registered = self
            .registered
            .write()
            .expect("registered spaces")
            .remove(&space.skey);
        let hosted = self
            .hosted
            .write()
            .expect("hosted spaces")
            .remove(&space.skey);
        Ok(registered || hosted)
    }

    /// The simplespace config surfaced by `getSpace`.
    pub fn space_config(&self, policy: &Policy) -> SimplespaceConfig {
        let app_access = match &self.app_access {
            AppAccess::Open => LexAppAccess::Open(AppAccessOpen {}),
            AppAccess::AllowList(allowed) => LexAppAccess::AllowList(AppAccessAllowList {
                allowed: allowed.clone(),
            }),
        };
        SimplespaceConfig {
            policy: Some(policy.lexicon_policy()),
            app_access: Some(app_access),
            managing_app: policy.managing_app().map(str::to_string),
        }
    }

    /// Mint a space credential for an already-authorized user (2h, no `aud`,
    /// signed by the authority's space key), bound to `dpop_jkt`. `jti` is
    /// caller-provided so the method stays deterministic/testable.
    ///
    /// `dpop_jkt` must come from a DPoP proof this server verified, never from
    /// a request field: a field is an assertion anyone holding a delegation
    /// token can make about a key someone else controls.
    pub fn mint_credential_for(
        &self,
        space: &SpaceId,
        now: u64,
        jti: String,
        dpop_jkt: &str,
    ) -> Result<String> {
        let header = JwtHeader {
            typ: CREDENTIAL_TYP.to_string(),
            alg: rsky_crypto::constants::SECP256K1_JWT_ALG.to_string(),
            kid: Some("#atproto_space".to_string()),
        };
        let claims = SpaceClaims {
            iss: self.authority_did().to_string(),
            sub: space.uri(),
            aud: None,
            iat: now,
            exp: now + CREDENTIAL_TTL_SECS,
            jti,
            cnf: Some(Confirmation {
                jkt: dpop_jkt.to_string(),
            }),
        };
        let jwt = credential::encode(&header, &claims, |input| self.signer.sign(input))?;
        Ok(jwt)
    }

    pub fn mint_credential(&self, now: u64, jti: String, dpop_jkt: &str) -> Result<String> {
        self.mint_credential_for(&self.space, now, jti, dpop_jkt)
    }

    /// The full `getSpaceCredential` flow: verify the client attestation (when
    /// required or presented), apply appAccess, verify the delegation token,
    /// consult the policy, then mint.
    #[allow(clippy::too_many_arguments)]
    pub async fn get_space_credential_for(
        &self,
        space: &SpaceId,
        delegation_jwt: &str,
        attestation_jwt: Option<&str>,
        policy: &Policy,
        keys: &dyn KeyResolver,
        metadata: &dyn MetadataFetcher,
        jti_store: &dyn JtiStore,
        now: u64,
        jti: String,
        dpop_jkt: &str,
    ) -> Result<String> {
        // App axis: the attested client_id is only trustworthy after the
        // attestation's signature has been verified against the client's
        // published JWKS; a bare header value is never consulted.
        let attested_client_id = match attestation_jwt {
            Some(jwt) => Some(
                verify_client_attestation(jwt, self.authority_did(), metadata, jti_store, now)
                    .await?,
            ),
            None if self.app_access.requires_attestation() => {
                return Err(HostError::AttestationRequired);
            }
            None => None,
        };
        if !self.app_access.permits(attested_client_id.as_deref()) {
            return Err(HostError::ClientNotAuthorized);
        }
        // Verify the delegation token: resolve the user's key, check typ/sub/aud/exp/sig.
        let decoded =
            credential::decode(delegation_jwt).map_err(|e| HostError::Delegation(e.to_string()))?;
        let user_did = decoded.claims.iss.clone();
        let user_key = keys.service_key(&user_did).await?;
        let verified_user = credential::verify_delegation_token(
            delegation_jwt,
            &space.uri(),
            self.authority_did(),
            &user_key,
            now,
        )
        .map_err(|e| HostError::Delegation(e.to_string()))?;
        // User axis: the policy decision (member list, public, or managing app).
        if !policy
            .authorizes(&space.uri(), &verified_user, attested_client_id.as_deref())
            .await?
        {
            return Err(HostError::NotAuthorized);
        }
        self.mint_credential_for(space, now, jti, dpop_jkt)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn get_space_credential(
        &self,
        delegation_jwt: &str,
        attestation_jwt: Option<&str>,
        policy: &Policy,
        keys: &dyn KeyResolver,
        metadata: &dyn MetadataFetcher,
        jti_store: &dyn JtiStore,
        now: u64,
        jti: String,
        dpop_jkt: &str,
    ) -> Result<String> {
        self.get_space_credential_for(
            &self.space,
            delegation_jwt,
            attestation_jwt,
            policy,
            keys,
            metadata,
            jti_store,
            now,
            jti,
            dpop_jkt,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attestation::{ClientMetadata, InMemoryJtiStore};
    use crate::membership::InMemoryMembership;
    use crate::signing::test_signer;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use rsky_lexicon::com::atproto::simplespace::Policy as LexPolicy;
    use rsky_space::jwk::{EcJwk, JwkSet};
    use std::sync::Arc;

    const CLIENT_ID: &str = "https://blacksky.community/client-metadata.json";

    fn authority() -> Authority {
        let space = SpaceId::new(
            "did:plc:communityauthority",
            "community.blacksky.feed",
            "main",
        );
        Authority::new(space, test_signer(), AppAccess::Open)
    }

    #[test]
    fn registering_a_space_makes_it_immediately_serveable() {
        let authority = authority();
        let space = "at://did:plc:communityauthority/space/community.blacksky.feed/new";

        assert!(authority.register(space).unwrap());
        assert_eq!(authority.resolve(space).unwrap().skey, "new");
        assert!(authority.spaces().contains(&space.to_string()));
    }

    fn member_policy(dids: &[&str]) -> Policy {
        Policy::MemberList(Arc::new(InMemoryMembership::new(
            dids.iter().map(|d| d.to_string()),
        )))
    }

    fn client_key() -> p256::ecdsa::SigningKey {
        p256::ecdsa::SigningKey::from_slice(&[0x71u8; 32]).unwrap()
    }

    fn client_jwk() -> EcJwk {
        let point = client_key().verifying_key().to_encoded_point(false);
        let bytes = point.as_bytes();
        EcJwk {
            kty: "EC".to_string(),
            crv: "P-256".to_string(),
            x: URL_SAFE_NO_PAD.encode(&bytes[1..33]),
            y: URL_SAFE_NO_PAD.encode(&bytes[33..65]),
            kid: Some("key-1".to_string()),
        }
    }

    struct InlineJwksFetcher;
    #[async_trait]
    impl MetadataFetcher for InlineJwksFetcher {
        async fn client_metadata(&self, client_id: &str) -> Result<ClientMetadata> {
            Ok(ClientMetadata {
                client_id: client_id.to_string(),
                jwks: Some(JwkSet {
                    keys: vec![client_jwk()],
                }),
                jwks_uri: None,
            })
        }
        async fn jwks(&self, _url: &str) -> Result<JwkSet> {
            Err(HostError::Attestation("not used".into()))
        }
    }

    fn attestation_jwt(auth: &Authority, iat: u64) -> String {
        use p256::ecdsa::signature::hazmat::PrehashSigner;
        use sha2::Digest;
        let header = JwtHeader {
            typ: rsky_space::credential::ATTESTATION_TYP.to_string(),
            alg: "ES256".to_string(),
            kid: Some("key-1".to_string()),
        };
        let claims = SpaceClaims {
            iss: CLIENT_ID.to_string(),
            sub: CLIENT_ID.to_string(),
            aud: Some(format!("{}#atproto_space_host", auth.authority_did())),
            iat,
            exp: iat + 60,
            jti: "attest-jti".to_string(),
            cnf: None,
        };
        credential::encode(&header, &claims, |input| {
            let digest = sha2::Sha256::digest(input);
            let sig: p256::ecdsa::Signature =
                client_key().sign_prehash(&digest).expect("p256 signs");
            let sig = sig.normalize_s().unwrap_or(sig);
            Ok(sig.to_vec())
        })
        .unwrap()
    }

    #[test]
    fn mint_credential_is_verifiable() {
        let auth = authority();
        // Point the authority DID at its own signing key so the credential's
        // iss/sub/typ and signature all validate against the space key.
        let space_uri = auth.space_uri();
        let jwt = auth
            .mint_credential(1000, "jti-1".to_string(), "test-jkt")
            .unwrap();
        let did_key = auth.signer.did_key();
        // A syncer verifies the credential against the authority's space key.
        credential::verify_space_credential(&jwt, &space_uri, auth.authority_did(), did_key, 1000)
            .expect("freshly minted credential must verify");
        // Expired check.
        assert!(matches!(
            credential::verify_space_credential(
                &jwt,
                &space_uri,
                auth.authority_did(),
                did_key,
                99_999
            ),
            Err(rsky_space::SpaceError::Expired)
        ));
    }

    struct DenyAllKeys;
    #[async_trait]
    impl KeyResolver for DenyAllKeys {
        async fn service_key(&self, _did: &str) -> Result<String> {
            Err(HostError::Membership("no key".into()))
        }
    }

    struct FixedKey(String);
    #[async_trait]
    impl KeyResolver for FixedKey {
        async fn service_key(&self, _did: &str) -> Result<String> {
            Ok(self.0.clone())
        }
    }

    /// A delegation token signed by a real user key for the authority's space.
    fn user_delegation(auth: &Authority, user_did: &str, iat: u64) -> (String, String) {
        use rsky_space::credential::{encode, JwtHeader, SpaceClaims, DELEGATION_TYP};
        let user_signer =
            Signer::from_secret(secp256k1::SecretKey::from_slice(&[0x77u8; 32]).unwrap());
        let header = JwtHeader {
            typ: DELEGATION_TYP.to_string(),
            alg: rsky_crypto::constants::SECP256K1_JWT_ALG.to_string(),
            kid: Some("#atproto".to_string()),
        };
        let claims = SpaceClaims {
            iss: user_did.to_string(),
            sub: auth.space_uri(),
            aud: Some(format!("{}#atproto_space_host", auth.authority_did())),
            iat,
            exp: iat + 60,
            jti: "delegation-jti".to_string(),
            cnf: None,
        };
        let jwt = encode(&header, &claims, |input| user_signer.sign(input)).unwrap();
        (jwt, user_signer.did_key().to_string())
    }

    #[tokio::test]
    async fn key_resolution_failure_propagates() {
        let auth = authority();
        let user = "did:plc:member";
        let (jwt, _) = user_delegation(&auth, user, 1000);
        let policy = member_policy(&[user]);
        let res = auth
            .get_space_credential(
                &jwt,
                None,
                &policy,
                &DenyAllKeys,
                &InlineJwksFetcher,
                &InMemoryJtiStore::default(),
                1000,
                "jti".into(),
                "test-jkt",
            )
            .await;
        assert!(matches!(res, Err(HostError::Membership(_))));
    }

    #[tokio::test]
    async fn full_mint_flow_authorizes_member() {
        let auth = authority();
        let user = "did:plc:member";
        let (jwt, user_key) = user_delegation(&auth, user, 1000);
        let policy = member_policy(&[user]);

        let credential = auth
            .get_space_credential(
                &jwt,
                None,
                &policy,
                &FixedKey(user_key),
                &InlineJwksFetcher,
                &InMemoryJtiStore::default(),
                1000,
                "jti".into(),
                "test-jkt",
            )
            .await
            .expect("member with valid delegation gets a credential");
        credential::verify_space_credential(
            &credential,
            &auth.space_uri(),
            auth.authority_did(),
            auth.signer.did_key(),
            1000,
        )
        .expect("minted credential verifies against the space key");
    }

    #[tokio::test]
    async fn non_member_is_denied() {
        let auth = authority();
        let (jwt, user_key) = user_delegation(&auth, "did:plc:stranger", 1000);
        let policy = member_policy(&[]);
        let res = auth
            .get_space_credential(
                &jwt,
                None,
                &policy,
                &FixedKey(user_key),
                &InlineJwksFetcher,
                &InMemoryJtiStore::default(),
                1000,
                "jti".into(),
                "test-jkt",
            )
            .await;
        assert!(matches!(res, Err(HostError::NotAuthorized)));
    }

    #[tokio::test]
    async fn expired_or_garbage_delegation_is_denied() {
        let auth = authority();
        let user = "did:plc:member";
        let (jwt, user_key) = user_delegation(&auth, user, 1000);
        let policy = member_policy(&[user]);
        let res = auth
            .get_space_credential(
                &jwt,
                None,
                &policy,
                &FixedKey(user_key.clone()),
                &InlineJwksFetcher,
                &InMemoryJtiStore::default(),
                5000,
                "jti".into(),
                "test-jkt",
            )
            .await;
        assert!(matches!(res, Err(HostError::Delegation(_))));

        let res = auth
            .get_space_credential(
                "a.b",
                None,
                &policy,
                &FixedKey(user_key),
                &InlineJwksFetcher,
                &InMemoryJtiStore::default(),
                1000,
                "jti".into(),
                "test-jkt",
            )
            .await;
        assert!(matches!(res, Err(HostError::Delegation(_))));
    }

    #[tokio::test]
    async fn space_config_surfaces_policy_and_app_access() {
        let auth = authority();
        let policy = Policy::Public;
        let cfg = auth.space_config(&policy);
        assert_eq!(cfg.policy, Some(LexPolicy::Public));
        assert!(matches!(cfg.app_access, Some(LexAppAccess::Open(_))));
        assert!(cfg.managing_app.is_none());

        let space = SpaceId::new("did:plc:auth", "community.blacksky.feed", "main");
        let auth = Authority::new(
            space,
            test_signer(),
            AppAccess::AllowList(vec![CLIENT_ID.to_string()]),
        );
        struct NeverApp;
        #[async_trait]
        impl crate::managing_app::ManagingAppClient for NeverApp {
            async fn check_user_access(&self, _: &str, _: &str, _: Option<&str>) -> Result<bool> {
                Ok(false)
            }
        }
        let policy = Policy::ManagingApp {
            service_id: "did:web:app.example#managing_app".to_string(),
            client: Arc::new(NeverApp),
        };
        let cfg = auth.space_config(&policy);
        assert_eq!(cfg.policy, Some(LexPolicy::ManagingApp));
        assert!(
            matches!(cfg.app_access, Some(LexAppAccess::AllowList(list)) if list.allowed == vec![CLIENT_ID.to_string()])
        );
        assert_eq!(
            cfg.managing_app.as_deref(),
            Some("did:web:app.example#managing_app")
        );
        assert!(!policy.authorizes("space", "did:plc:u", None).await.unwrap());
    }

    #[tokio::test]
    async fn allowlist_without_attestation_requires_one() {
        let space = SpaceId::new("did:plc:auth", "community.blacksky.feed", "main");
        let auth = Authority::new(
            space,
            test_signer(),
            AppAccess::AllowList(vec![CLIENT_ID.to_string()]),
        );
        let policy = member_policy(&["did:plc:user"]);
        let res = auth
            .get_space_credential(
                "a.b.c",
                None,
                &policy,
                &DenyAllKeys,
                &InlineJwksFetcher,
                &InMemoryJtiStore::default(),
                1000,
                "jti".into(),
                "test-jkt",
            )
            .await;
        assert!(matches!(res, Err(HostError::AttestationRequired)));
    }

    #[tokio::test]
    async fn allowlist_rejects_unlisted_attested_client() {
        let space = SpaceId::new("did:plc:auth", "community.blacksky.feed", "main");
        let auth = Authority::new(
            space,
            test_signer(),
            AppAccess::AllowList(vec!["https://other.example/client".to_string()]),
        );
        let policy = member_policy(&["did:plc:user"]);
        // The attestation verifies (proving the client is CLIENT_ID), but that
        // client is not on the allow list.
        let attest = attestation_jwt(&auth, 1000);
        let res = auth
            .get_space_credential(
                "a.b.c",
                Some(&attest),
                &policy,
                &DenyAllKeys,
                &InlineJwksFetcher,
                &InMemoryJtiStore::default(),
                1000,
                "jti".into(),
                "test-jkt",
            )
            .await;
        assert!(matches!(res, Err(HostError::ClientNotAuthorized)));
    }

    #[tokio::test]
    async fn allowlisted_attested_client_mints() {
        let space = SpaceId::new("did:plc:auth", "community.blacksky.feed", "main");
        let auth = Authority::new(
            space,
            test_signer(),
            AppAccess::AllowList(vec![CLIENT_ID.to_string()]),
        );
        let user = "did:plc:member";
        let (delegation, user_key) = user_delegation(&auth, user, 1000);
        let attest = attestation_jwt(&auth, 1000);
        let policy = member_policy(&[user]);
        let credential = auth
            .get_space_credential(
                &delegation,
                Some(&attest),
                &policy,
                &FixedKey(user_key),
                &InlineJwksFetcher,
                &InMemoryJtiStore::default(),
                1000,
                "jti".into(),
                "test-jkt",
            )
            .await
            .expect("attested, allow-listed client mints for a member");
        credential::verify_space_credential(
            &credential,
            &auth.space_uri(),
            auth.authority_did(),
            auth.signer.did_key(),
            1000,
        )
        .unwrap();
    }

    #[tokio::test]
    async fn test_doubles_behave_as_declared() {
        assert!(InlineJwksFetcher.jwks("unused").await.is_err());
        assert!(InlineJwksFetcher
            .client_metadata(CLIENT_ID)
            .await
            .unwrap()
            .jwks
            .is_some());
    }

    #[tokio::test]
    async fn invalid_attestation_is_rejected_even_for_open_access() {
        // An attestation presented voluntarily is still verified.
        let auth = authority();
        let policy = member_policy(&["did:plc:member"]);
        let res = auth
            .get_space_credential(
                "a.b.c",
                Some("garbage"),
                &policy,
                &DenyAllKeys,
                &InlineJwksFetcher,
                &InMemoryJtiStore::default(),
                1000,
                "jti".into(),
                "test-jkt",
            )
            .await;
        assert!(matches!(res, Err(HostError::Attestation(_))));
    }
}
