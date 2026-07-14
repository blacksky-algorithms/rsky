//! The space authority: mints space credentials under the managing-app policy.

use async_trait::async_trait;
use rsky_space::credential::{self, JwtHeader, SpaceClaims, CREDENTIAL_TTL_SECS, CREDENTIAL_TYP};
use rsky_space::space_id::SpaceId;

use crate::appaccess::AppAccess;
use crate::error::{HostError, Result};
use crate::membership::MembershipStore;
use crate::signing::Signer;

/// Resolves an account's atproto signing `did:key` (from its DID document), used
/// to verify a delegation token minted by that user's PDS.
#[async_trait]
pub trait KeyResolver: Send + Sync {
    async fn signing_key(&self, did: &str) -> Result<String>;
}

/// A space authority for a single space.
pub struct Authority {
    pub space: SpaceId,
    pub signer: Signer,
    pub app_access: AppAccess,
}

/// The space config surfaced by `getSpace`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SpaceConfig {
    pub space: String,
    pub policy: &'static str,
    pub managing_app: String,
    pub requires_attestation: bool,
}

impl Authority {
    pub fn new(space: SpaceId, signer: Signer, app_access: AppAccess) -> Self {
        Self {
            space,
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

    pub fn space_config(&self, managing_app: &str) -> SpaceConfig {
        SpaceConfig {
            space: self.space_uri(),
            policy: "managing-app",
            managing_app: managing_app.to_string(),
            requires_attestation: self.app_access.requires_attestation(),
        }
    }

    /// Mint a space credential for an already-authorized user (2h, no `aud`,
    /// signed by the authority's space key). `jti` is caller-provided so the
    /// method stays deterministic/testable.
    pub fn mint_credential(&self, now: u64, jti: String) -> Result<String> {
        let header = JwtHeader {
            typ: CREDENTIAL_TYP.to_string(),
            alg: rsky_crypto::constants::SECP256K1_JWT_ALG.to_string(),
            kid: Some("#atproto_space".to_string()),
        };
        let claims = SpaceClaims {
            iss: self.authority_did().to_string(),
            sub: self.space_uri(),
            aud: None,
            iat: now,
            exp: now + CREDENTIAL_TTL_SECS,
            jti,
        };
        let jwt = credential::encode(&header, &claims, |input| self.signer.sign(input))?;
        Ok(jwt)
    }

    /// The full `getSpaceCredential` flow: verify the delegation token, apply
    /// the managing-app (user) and appAccess (app) checks, then mint.
    #[allow(clippy::too_many_arguments)]
    pub async fn get_space_credential(
        &self,
        delegation_jwt: &str,
        attested_client_id: Option<&str>,
        membership: &dyn MembershipStore,
        keys: &dyn KeyResolver,
        now: u64,
        jti: String,
    ) -> Result<String> {
        // App axis first (cheap, no network): reject disallowed clients.
        if !self.app_access.permits(attested_client_id) {
            return Err(HostError::ClientNotAuthorized);
        }
        // Verify the delegation token: resolve the user's key, check typ/sub/aud/exp/sig.
        let decoded = credential::decode(delegation_jwt)?;
        let user_did = decoded.claims.iss.clone();
        let user_key = keys.signing_key(&user_did).await?;
        let verified_user = credential::verify_delegation_token(
            delegation_jwt,
            &self.space_uri(),
            self.authority_did(),
            &user_key,
            now,
        )
        .map_err(|e| HostError::Delegation(e.to_string()))?;
        // Managing-app (user) axis: membership decision.
        if !membership.is_member(&verified_user).await? {
            return Err(HostError::NotAuthorized);
        }
        self.mint_credential(now, jti)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::membership::InMemoryMembership;
    use crate::signing::test_signer;

    fn authority() -> Authority {
        let space = SpaceId::new(
            "did:plc:communityauthority",
            "community.blacksky.feed",
            "main",
        );
        Authority::new(space, test_signer(), AppAccess::Open)
    }

    #[test]
    fn mint_credential_is_verifiable() {
        let auth = authority();
        // Point the authority DID at its own signing key so the credential's
        // iss/sub/typ and signature all validate against the space key.
        let space_uri = auth.space_uri();
        let jwt = auth.mint_credential(1000, "jti-1".to_string()).unwrap();
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
        async fn signing_key(&self, _did: &str) -> Result<String> {
            Err(HostError::Membership("no key".into()))
        }
    }

    struct FixedKey(String);
    #[async_trait]
    impl KeyResolver for FixedKey {
        async fn signing_key(&self, _did: &str) -> Result<String> {
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
        };
        let jwt = encode(&header, &claims, |input| user_signer.sign(input)).unwrap();
        (jwt, user_signer.did_key().to_string())
    }

    #[tokio::test]
    async fn key_resolution_failure_propagates() {
        let auth = authority();
        let user = "did:plc:member";
        let (jwt, _) = user_delegation(&auth, user, 1000);
        let members = InMemoryMembership::new([user.to_string()]);
        let res = auth
            .get_space_credential(&jwt, None, &members, &DenyAllKeys, 1000, "jti".into())
            .await;
        assert!(matches!(res, Err(HostError::Membership(_))));
    }

    #[tokio::test]
    async fn full_mint_flow_authorizes_member() {
        let auth = authority();
        let user = "did:plc:member";
        let (jwt, user_key) = user_delegation(&auth, user, 1000);
        let members = InMemoryMembership::new([user.to_string()]);

        let credential = auth
            .get_space_credential(
                &jwt,
                None,
                &members,
                &FixedKey(user_key),
                1000,
                "jti".into(),
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
        let members = InMemoryMembership::default();
        let res = auth
            .get_space_credential(
                &jwt,
                None,
                &members,
                &FixedKey(user_key),
                1000,
                "jti".into(),
            )
            .await;
        assert!(matches!(res, Err(HostError::NotAuthorized)));
    }

    #[tokio::test]
    async fn expired_delegation_is_denied() {
        let auth = authority();
        let user = "did:plc:member";
        let (jwt, user_key) = user_delegation(&auth, user, 1000);
        let members = InMemoryMembership::new([user.to_string()]);
        let res = auth
            .get_space_credential(
                &jwt,
                None,
                &members,
                &FixedKey(user_key),
                5000,
                "jti".into(),
            )
            .await;
        assert!(matches!(res, Err(HostError::Delegation(_))));
    }

    #[test]
    fn space_config_surfaces_policy() {
        let auth = authority();
        let cfg = auth.space_config("did:web:app.example#managing_app");
        assert_eq!(cfg.policy, "managing-app");
        assert_eq!(cfg.space, auth.space_uri());
        assert!(!cfg.requires_attestation);
    }

    #[tokio::test]
    async fn allowlist_rejects_unknown_client_before_network() {
        let space = SpaceId::new("did:plc:auth", "community.blacksky.feed", "main");
        let auth = Authority::new(
            space,
            test_signer(),
            AppAccess::AllowList(vec!["https://blacksky.community/client".into()]),
        );
        let members = InMemoryMembership::new(["did:plc:user".to_string()]);
        // A disallowed client is rejected without ever resolving a key.
        let res = auth
            .get_space_credential(
                "a.b.c",
                Some("https://evil.example/client"),
                &members,
                &DenyAllKeys,
                1000,
                "jti".into(),
            )
            .await;
        assert!(matches!(res, Err(HostError::ClientNotAuthorized)));
    }
}
