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
