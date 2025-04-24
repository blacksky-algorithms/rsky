use crate::jwk::{Keyset, SignedJwt};
use crate::oauth_provider::access_token::access_token_type::AccessTokenType;
use crate::oauth_provider::dpop::dpop_manager::{DpopManager, DpopManagerOptions};
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::lib::util::authorization_header::AuthorizationHeader;
use crate::oauth_provider::replay::replay_manager::ReplayManager;
use crate::oauth_provider::replay::replay_store::ReplayStore;
use crate::oauth_provider::replay::replay_store_memory::ReplayStoreMemory;
use crate::oauth_provider::signer::signer::Signer;
use crate::oauth_provider::token::verify_token_claims::{
    verify_token_claims, VerifyTokenClaimsOptions, VerifyTokenClaimsResult,
};
use crate::oauth_types::{OAuthAccessToken, OAuthIssuerIdentifier, OAuthTokenType};
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct OAuthVerifierOptions {
    /**
     * The "issuer" identifier of the OAuth provider, this is the base URL of the
     * OAuth provider.
     */
    pub issuer: OAuthIssuerIdentifier,
    /**
     * The keyset used to sign access tokens.
     */
    pub keyset: Arc<RwLock<Keyset>>,
    /**
     * If set to {@link AccessTokenType.jwt}, the provider will use JWTs for
     * access tokens. If set to {@link AccessTokenType.id}, the provider will
     * use tokenId as access tokens. If set to {@link AccessTokenType.auto},
     * JWTs will only be used if the audience is different from the issuer.
     * Defaults to {@link AccessTokenType.jwt}.
     *
     * Here is a comparison of the two types:
     *
     * - pro id: less CPU intensive (no crypto operations)
     * - pro id: less bandwidth (shorter tokens than jwt)
     * - pro id: token data is in sync with database (e.g. revocation)
     * - pro jwt: stateless: no I/O needed (no db lookups through token store)
     * - pro jwt: stateless: allows Resource Server to be on a different
     *   host/server
     */
    pub access_token_type: Option<AccessTokenType>,
    /**
     * A redis instance to use for replay protection. If not provided, replay
     * protection will use memory storage.
     */
    pub redis: Option<String>,
    pub replay_store: Option<Arc<RwLock<dyn ReplayStore>>>,
    pub dpop_options: Option<DpopManagerOptions>,
}

pub struct OAuthVerifier {
    pub issuer: OAuthIssuerIdentifier,
    pub keyset: Arc<RwLock<Keyset>>,
    pub access_token_type: AccessTokenType,
    pub dpop_manager: DpopManager,
    pub replay_manager: ReplayManager,
    pub signer: Arc<RwLock<Signer>>,
    pub redis: Option<String>,
}

impl OAuthVerifier {
    pub fn new(opts: OAuthVerifierOptions) -> Self {
        let replay_store = match opts.replay_store {
            None => match opts.redis {
                None => Arc::new(RwLock::new(ReplayStoreMemory::new())),
                Some(_redis) => {
                    unimplemented!()
                }
            },
            Some(replay_store) => replay_store,
        };
        OAuthVerifier {
            issuer: opts.issuer.clone(),
            keyset: opts.keyset.clone(),
            access_token_type: opts.access_token_type.unwrap_or(AccessTokenType::JWT),
            dpop_manager: DpopManager::new(opts.dpop_options).unwrap(),
            replay_manager: ReplayManager::new(replay_store),
            signer: Arc::new(RwLock::new(Signer::new(
                opts.issuer.clone(),
                opts.keyset.clone(),
            ))),
            redis: None,
        }
    }

    pub async fn next_dpop_nonce(&mut self) -> Option<String> {
        self.dpop_manager.next_nonce().await
    }

    pub async fn check_dpop_proof(
        &mut self,
        proof: &str,
        htm: &str, // HTTP Method
        htu: &str, // HTTP URL
        access_token: Option<OAuthAccessToken>,
    ) -> Result<String, OAuthError> {
        let res = self
            .dpop_manager
            .check_proof(proof, htm, htu, access_token)
            .await?;

        let unique = self.replay_manager.unique_dpop(res.jti, None).await;
        if !unique {
            return Err(OAuthError::InvalidDpopProofError(
                "DPoP proof jti is not unique".to_string(),
            ));
        }

        Ok(res.jkt)
    }

    pub fn assert_token_type_allowed(
        &self,
        token_type: OAuthTokenType,
        access_token_type: AccessTokenType,
    ) -> Result<(), OAuthError> {
        if self.access_token_type != AccessTokenType::AUTO
            && self.access_token_type != access_token_type
        {
            return Err(OAuthError::InvalidTokenError(
                token_type,
                "Invalid token type".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn authenticate_token(
        &self,
        token_type: OAuthTokenType,
        token: OAuthAccessToken,
        dpop_jkt: Option<String>,
        verify_options: Option<VerifyTokenClaimsOptions>,
    ) -> Result<VerifyTokenClaimsResult, OAuthError> {
        println!("{}", token.clone().into_inner());
        let signed_jwt = match SignedJwt::new(token.clone().into_inner()) {
            Ok(signed_jwt) => signed_jwt,
            Err(_) => {
                return Err(OAuthError::InvalidTokenError(
                    token_type,
                    "Malformed token".to_string(),
                ));
            }
        };

        self.assert_token_type_allowed(token_type.clone(), AccessTokenType::JWT)?;

        let signer = self.signer.read().await;
        let payload = signer
            .verify_access_token(signed_jwt.clone(), None)
            .await?
            .payload;

        verify_token_claims(
            token,
            payload.jti.clone(),
            token_type,
            dpop_jkt,
            payload.as_token_claims(),
            verify_options,
        )
    }

    pub async fn authenticate_request(
        &mut self,
        method: String,
        url: String,
        headers: (Option<&str>, Option<&str>),
        verify_options: Option<VerifyTokenClaimsOptions>,
    ) -> Result<VerifyTokenClaimsResult, OAuthError> {
        let authorization_header = match AuthorizationHeader::new(headers.0.unwrap()) {
            Ok(authorization_header) => authorization_header,
            Err(_) => {
                return Err(OAuthError::RuntimeError(
                    "Failed to get AuthorizationHeader".to_string(),
                ))
            }
        };
        let token_type = authorization_header.token_type;
        let token = authorization_header.oauth_access_token;

        let dpop_jkt = self
            .check_dpop_proof(
                headers.1.unwrap().as_str(),
                method.as_str(),
                url.as_str(),
                Some(token.clone()),
            )
            .await?;

        self.authenticate_token(token_type, token, Some(dpop_jkt), verify_options)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jwk::Key;
    use crate::jwk_jose::jose_key::JoseKey;
    use crate::oauth_provider::dpop::dpop_nonce::DpopNonceInput;
    use crate::oauth_provider::token::token_id::TokenId;
    use biscuit::jwa;
    use biscuit::jwa::Algorithm;
    use biscuit::jwk::{
        AlgorithmParameters, CommonParameters, EllipticCurveKeyParameters, EllipticCurveKeyType,
        KeyOperations, PublicKeyUse, RSAKeyParameters, JWK,
    };
    use num_bigint::BigUint;

    async fn build_keyset() -> Keyset {
        let mut keys = Vec::new();
        let jwk = JWK {
            common: CommonParameters {
                algorithm: Some(Algorithm::Signature(jwa::SignatureAlgorithm::RS256)),
                key_id: Some("2011-04-29".to_string()),
                ..Default::default()
            },
            algorithm: AlgorithmParameters::RSA(RSAKeyParameters {
                n: BigUint::new(vec![
                    2661337731, 446995658, 1209332140, 183172752, 955894533, 3140848734, 581365968,
                    3217299938, 3520742369, 1559833632, 1548159735, 2303031139, 1726816051,
                    92775838, 37272772, 1817499268, 2876656510, 1328166076, 2779910671, 4258539214,
                    2834014041, 3172137349, 4008354576, 121660540, 1941402830, 1620936445,
                    993798294, 47616683, 272681116, 983097263, 225284287, 3494334405, 4005126248,
                    1126447551, 2189379704, 4098746126, 3730484719, 3232696701, 2583545877,
                    428738419, 2533069420, 2922211325, 2227907999, 4154608099, 679827337,
                    1165541732, 2407118218, 3485541440, 799756961, 1854157941, 3062830172,
                    3270332715, 1431293619, 3068067851, 2238478449, 2704523019, 2826966453,
                    1548381401, 3719104923, 2605577849, 2293389158, 273345423, 169765991,
                    3539762026,
                ]),
                e: BigUint::new(vec![65537]),
                ..Default::default()
            }),
            additional: Default::default(),
        };
        let key = JoseKey::from_jwk(jwk, None).await;
        keys.push(Box::new(key) as Box<dyn Key>);
        Keyset::new(keys)
    }

    async fn create_oauth_verifier() -> OAuthVerifier {
        let keyset = Arc::new(RwLock::new(build_keyset().await));
        let opts = OAuthVerifierOptions {
            issuer: OAuthIssuerIdentifier::new("https://pds.ripperoni.com".to_string()).unwrap(),
            keyset,
            access_token_type: None,
            redis: None,
            replay_store: None,
            dpop_options: Some(DpopManagerOptions {
                dpop_secret: Some(DpopNonceInput::String(
                    "1c9d92bea9a498e6165a39473e724a5d1c9d92bea9a498e6165a39473e724a5d".to_string(),
                )),
                dpop_step: Some(1),
            }),
        };
        OAuthVerifier::new(opts)
    }

    #[tokio::test]
    async fn test_next_dpop_nonce() {
        let mut oauth_verifier = create_oauth_verifier().await;
        let result = oauth_verifier.next_dpop_nonce();
    }

    #[tokio::test]
    async fn test_check_dpop_proof() {
        let mut oauth_verifier = create_oauth_verifier().await;
        let proof: &str = "eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiJhTjlOSVJGUWNYZFZJZy16SFBHOXZYYVM1ZkRYbWlhWTZfU0t2SXhDTGxFIiwieSI6Ik0waUtwdUJRYVJqUm9fWGtBRmRjMEdnUWVxendJNl9YVEtHa2ZMTjNIRk0ifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ0NDI3MTIyLCJqdGkiOiJoNmNpcjh2MWl3OjFqbW9zZnA0a29tZXoiLCJodG0iOiJQT1NUIiwiaHR1IjoiaHR0cHM6Ly9wZHMucmlwcGVyb25pLmNvbS9vYXV0aC9wYXIiLCJub25jZSI6IlpzTko0eV9RclJ3dzRVZE9aV2p4RERpa3doczF6emJqT19fb1VKNmJxRm8ifQ.YN7sVm3Hj9PAxCzG6Ql_FqDSpDJUYbibBFUgKXahGVFY9NojUeD67D0dUvlmZYy2e7slAtQhxqzFC9Nvly0SWA";
        let htm: &str = "POST";
        let htu: &str = "https://pds.ripperoni.com/oauth/par";
        let access_token: Option<OAuthAccessToken> = None;
        let result = oauth_verifier
            .check_dpop_proof(proof, htm, htu, access_token)
            .await
            .unwrap();
        let expected = String::from("eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiJhTjlOSVJGUWNYZFZJZy16SFBHOXZYYVM1ZkRYbWlhWTZfU0t2SXhDTGxFIiwieSI6Ik0waUtwdUJRYVJqUm9fWGtBRmRjMEdnUWVxendJNl9YVEtHa2ZMTjNIRk0ifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ0NDI3MTIyLCJqdGkiOiJoNmNpcjh2MWl3OjFqbW9zZnA0a29tZXoiLCJodG0iOiJQT1NUIiwiaHR1IjoiaHR0cHM6Ly9wZHMucmlwcGVyb25pLmNvbS9vYXV0aC9wYXIiLCJub25jZSI6IlpzTko0eV9RclJ3dzRVZE9aV2p4RERpa3doczF6emJqT19fb1VKNmJxRm8ifQ.YN7sVm3Hj9PAxCzG6Ql_FqDSpDJUYbibBFUgKXahGVFY9NojUeD67D0dUvlmZYy2e7slAtQhxqzFC9Nvly0SWA");
        assert_eq!(result, expected)
    }

    #[tokio::test]
    async fn test_assert_token_type_allowed() {
        let oauth_verifier = create_oauth_verifier().await;
        let token_type = OAuthTokenType::DPoP;
        let access_token_type = AccessTokenType::JWT;
        oauth_verifier
            .assert_token_type_allowed(token_type, access_token_type)
            .unwrap();
    }

    #[tokio::test]
    async fn test_authenticate_token() {
        let oauth_verifier = create_oauth_verifier().await;
        let token_type = OAuthTokenType::DPoP;
        let token = OAuthAccessToken::new("eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiJhTjlOSVJGUWNYZFZJZy16SFBHOXZYYVM1ZkRYbWlhWTZfU0t2SXhDTGxFIiwieSI6Ik0waUtwdUJRYVJqUm9fWGtBRmRjMEdnUWVxendJNl9YVEtHa2ZMTjNIRk0ifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ0NDI3MTIyLCJqdGkiOiJoNmNpcjh2MWl3OjFqbW9zZnA0a29tZXoiLCJodG0iOiJQT1NUIiwiaHR1IjoiaHR0cHM6Ly9wZHMucmlwcGVyb25pLmNvbS9vYXV0aC9wYXIiLCJub25jZSI6IlpzTko0eV9RclJ3dzRVZE9aV2p4RERpa3doczF6emJqT19fb1VKNmJxRm8ifQ.YN7sVm3Hj9PAxCzG6Ql_FqDSpDJUYbibBFUgKXahGVFY9NojUeD67D0dUvlmZYy2e7slAtQhxqzFC9Nvly0SWA").unwrap();
        let dpop_jkt: Option<String> = Some("token".to_string());
        let verify_options: Option<VerifyTokenClaimsOptions> = None;
        let result = oauth_verifier
            .authenticate_token(token_type, token, dpop_jkt, verify_options)
            .await
            .unwrap();
        let expected = VerifyTokenClaimsResult {
            token: OAuthAccessToken::new("eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiJhTjlOSVJGUWNYZFZJZy16SFBHOXZYYVM1ZkRYbWlhWTZfU0t2SXhDTGxFIiwieSI6Ik0waUtwdUJRYVJqUm9fWGtBRmRjMEdnUWVxendJNl9YVEtHa2ZMTjNIRk0ifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ0NDI3MTIyLCJqdGkiOiJoNmNpcjh2MWl3OjFqbW9zZnA0a29tZXoiLCJodG0iOiJQT1NUIiwiaHR1IjoiaHR0cHM6Ly9wZHMucmlwcGVyb25pLmNvbS9vYXV0aC9wYXIiLCJub25jZSI6IlpzTko0eV9RclJ3dzRVZE9aV2p4RERpa3doczF6emJqT19fb1VKNmJxRm8ifQ.YN7sVm3Hj9PAxCzG6Ql_FqDSpDJUYbibBFUgKXahGVFY9NojUeD67D0dUvlmZYy2e7slAtQhxqzFC9Nvly0SWA").unwrap(),
            token_id: TokenId::new("tok-dwadwdaddwadwdad").unwrap(),
            token_type: OAuthTokenType::DPoP,
            claims: Default::default(),
        };
        assert_eq!(result, expected)
    }

    //TODO Fix nonce for testing
    #[tokio::test]
    async fn test_authenticate_request() {
        let mut oauth_verifier = create_oauth_verifier().await;
        let method: String = String::from("GET");
        let url: String = String::from("https://pds.ripperoni.com/xrpc/app.bsky.actor.getProfile?actor=did%3Aplc%3Aetm5inzxhy2wto26ggprzggs");
        let headers: (Option<&str>, Option<&str>) =
            (Some("DPoP tok-7bddad4f0bf4f788dfb71edc41b4247f"),
             Some("eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiJBMDRoR21uTnlSenlRN1U4TWYwdkltcG1QV2hVdi1QcEhYZ2dyRWpKNlUwIiwieSI6IkdWXzc0ekx6SDVqQkh1X3Z1T3hlTlhXNVNCSDZCM1RFTjl6UERUN0d1U3cifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ1MzA3MzE4LCJqdGkiOiJoNm5yNDJxbWJlOjM0bjgzb2hvMnJ3NWQiLCJodG0iOiJHRVQiLCJodHUiOiJodHRwczovL3Bkcy5yaXBwZXJvbmkuY29tL3hycGMvYXBwLmJza3kuYWN0b3IuZ2V0UHJvZmlsZT9hY3Rvcj1kaWQlM0FwbGMlM0FldG01aW56eGh5Mnd0bzI2Z2dwcnpnZ3MiLCJub25jZSI6Ik4wcm94eWtqQmJ6RzVJYjVGTkhvaVMtNFJXQjBaR3pldlJmbDhsOWYtUDgiLCJhdGgiOiJaVXFDLWtuR01zUzdTenI1SldoMUJwZ01MVTNiZ3lJcFRjZmhIdnZuQ0xzIn0.dyO0MS-7hBj_ru10IkyZcfbW0OhrfEvsCUmXBQFe74tamfAFqb86OFeSGERVwNNp1kodHzcp11Ffs_AUgYeU0w"));
        let verify_options: Option<VerifyTokenClaimsOptions> = None;
        let result = oauth_verifier
            .authenticate_request(method, url, headers, verify_options)
            .await
            .unwrap();
        let expected = VerifyTokenClaimsResult {
            token: OAuthAccessToken::new("tok-739361c165c76408088de74ee136cf66").unwrap(),
            token_id: TokenId::new("tok-739361c165c76408088de74ee136cf66").unwrap(),
            token_type: OAuthTokenType::DPoP,
            claims: Default::default(),
        };
        assert_eq!(result, expected)
    }
}
