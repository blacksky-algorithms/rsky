use crate::jwk::{
    Audience, JwkError, JwtConfirmation, JwtHeader, JwtPayload, Key, Keyset, SignedJwt,
    VerifyOptions, VerifyResult,
};
use crate::oauth_provider::client::client::Client;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::oidc::sub::Sub;
use crate::oauth_provider::signer::signed_token_payload::SignedTokenPayload;
use crate::oauth_provider::token::token_id::TokenId;
use crate::oauth_types::{
    OAuthAuthorizationDetails, OAuthAuthorizationRequestParameters, OAuthIssuerIdentifier,
};
use biscuit::jwa::Algorithm;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct Signer {
    pub issuer: OAuthIssuerIdentifier,
    pub keyset: Arc<RwLock<Keyset>>,
}

pub type SignerCreator = Box<dyn Fn(Arc<RwLock<Keyset>>) -> Signer + Send + Sync>;

impl Signer {
    pub fn creator(issuer: OAuthIssuerIdentifier) -> SignerCreator {
        Box::new(move |keyset: Arc<RwLock<Keyset>>| -> Signer {
            Signer::new(issuer.clone(), keyset)
        })
    }

    pub fn new(issuer: OAuthIssuerIdentifier, keyset: Arc<RwLock<Keyset>>) -> Self {
        Signer { issuer, keyset }
    }

    pub async fn verify(
        &self,
        signed_jwt: SignedJwt,
        verify_options: Option<VerifyOptions>,
    ) -> Result<VerifyResult, JwkError> {
        let keyset = self.keyset.read().await;
        let verify_options = match verify_options {
            None => None,
            Some(verify_options) => {
                let mut verify_options = verify_options.clone();
                verify_options.issuer = Some(self.issuer.clone());
                Some(verify_options)
            }
        };
        let result = keyset.verify_jwt(signed_jwt, verify_options).await;
        result
    }

    pub async fn sign(
        &self,
        algorithms: Option<Vec<Algorithm>>,
        search_kids: Option<Vec<String>>,
        sign_header: JwtHeader,
        payload: JwtPayload,
    ) -> Result<SignedJwt, JwkError> {
        let keyset = self.keyset.read().await;
        keyset
            .create_jwt(algorithms, search_kids, sign_header, payload)
            .await
    }

    pub async fn access_token(
        &self,
        client: Client,
        parameters: OAuthAuthorizationRequestParameters,
        options: AccessTokenOptions,
    ) -> Result<SignedJwt, JwkError> {
        let mut header = JwtHeader::default();
        header.typ = Some("at+jwt".to_string());

        let mut payload = JwtPayload::default();
        payload.aud = Some(options.aud);
        if let Some(iat) = options.iat {
            payload.iat = Some(iat.timestamp());
        }
        payload.exp = Some(options.exp.timestamp());
        payload.sub = Some(options.sub);
        payload.jti = Some(options.jti.val());
        payload.cnf = options.cnf;
        // // https://datatracker.ietf.org/doc/html/rfc8693#section-4.3
        payload.client_id = Some(client.id);
        payload.scope = parameters.scope;

        // payload.authorization_details = options.authorization_details.unwrap();

        let alg = match options.alg {
            None => None,
            Some(alg) => Some(vec![alg]),
        };
        self.sign(alg, None, header, payload).await
    }

    pub async fn verify_access_token(
        &self,
        token: SignedJwt,
        options: Option<VerifyOptions>,
    ) -> Result<VerifyAccessTokenResponse, OAuthError> {
        let options = match options {
            None => VerifyOptions::default(),
            Some(options) => {
                let mut options = options.clone();
                options.issuer = None;
                options
            }
        };
        let result = match self.verify(token, Some(options)).await {
            Ok(result) => result,
            Err(error) => return Err(OAuthError::InvalidRequestError(error.to_string())),
        };
        let protected_header = result.protected_header;

        if let Some(typ) = &protected_header.typ {
            if typ != "at+jwt" {
                return Err(OAuthError::InvalidRequestError("".to_string()));
            }
        } else {
            return Err(OAuthError::InvalidRequestError("".to_string()));
        }

        let payload = match SignedTokenPayload::new(result.payload) {
            Ok(payload) => payload,
            Err(e) => return Err(OAuthError::InvalidRequestError("Bad payload".to_string())),
        };
        Ok(VerifyAccessTokenResponse {
            protected_header,
            payload,
        })
    }
}

pub struct AccessTokenOptions {
    pub aud: Audience,
    pub sub: Sub,
    pub jti: TokenId,
    pub exp: DateTime<Utc>,
    pub iat: Option<DateTime<Utc>>,
    pub alg: Option<Algorithm>,
    pub cnf: Option<JwtConfirmation>,
    pub authorization_details: Option<OAuthAuthorizationDetails>,
}

#[derive(Eq, PartialEq, Debug)]
pub struct VerifyAccessTokenResponse {
    pub protected_header: JwtHeader,
    pub payload: SignedTokenPayload,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jwk_jose::jose_key::JoseKey;
    use crate::oauth_types::{OAuthClientId, OAuthClientMetadata, OAuthResponseType};
    use biscuit::jwa;
    use biscuit::jwk::{AlgorithmParameters, CommonParameters, JWKSet, RSAKeyParameters, JWK};
    use biscuit::jws::Secret;
    use num_bigint::BigUint;

    #[tokio::test]
    async fn test_verify() {
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
        let jose_key = JoseKey::from_jwk(jwk, None).await;
        let issuer = OAuthIssuerIdentifier::new("http://pds.ripperoni.com").unwrap();
        let keyset = Keyset::new(vec![Box::new(jose_key)]);
        let keyset = Arc::new(RwLock::new(keyset));

        let token = SignedJwt::new("eyJ0eXAiOiJKV1QiLCJhbGciOiJSUzI1NiIsImtpZCI6Ik5FTXlNRUZDTXpVd01URTFRVE5CT1VGRE1FUTFPRGN6UmprNU56QkdRelk0UVRrMVEwWkVPUSJ9.eyJpc3MiOiJodHRwczovL2Rldi1lanRsOTg4dy5hdXRoMC5jb20vIiwic3ViIjoiZ1pTeXNwQ1k1ZEk0aDFaM3Fwd3BkYjlUNFVQZEdENWtAY2xpZW50cyIsImF1ZCI6Imh0dHA6Ly9oZWxsb3dvcmxkIiwiaWF0IjoxNTcyNDA2NDQ3LCJleHAiOjE1NzI0OTI4NDcsImF6cCI6ImdaU3lzcENZNWRJNGgxWjNxcHdwZGI5VDRVUGRHRDVrIiwiZ3R5IjoiY2xpZW50LWNyZWRlbnRpYWxzIn0.nupgm7iFqSnERq9GxszwBrsYrYfMuSfUGj8tGQlkY3Ksh3o_IDfq1GO5ngHQLZuYPD-8qPIovPBEVomGZCo_jYvsbjmYkalAStmF01TvSoXQgJd09ygZstH0liKsmINStiRE8fTA-yfEIuBYttROizx-cDoxiindbKNIGOsqf6yOxf7ww8DrTBJKYRnHVkAfIK8wm9LRpsaOVzWdC7S3cbhCKvANjT0RTRpAx8b_AOr_UCpOr8paj-xMT9Zc9HVCMZLBfj6OZ6yVvnC9g6q_SlTa--fY9SL5eqy6-q1JGoyK_-BQ_YrCwrRdrjoJsJ8j-XFRFWJX09W3oDuZ990nGA").unwrap();

        let signer = Signer::new(issuer, keyset);
        let verify_options = VerifyOptions {
            audience: Some("http://helloworld".to_string()),
            clock_tolerance: None,
            issuer: None,
            max_token_age: None,
            subject: None,
            typ: None,
            current_date: None,
            required_claims: vec![],
        };
        let result = signer.verify(token, Some(verify_options)).await;
    }

    #[tokio::test]
    async fn test_sign() {
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
        let jose_key = JoseKey::from_jwk(jwk, None).await;
        let issuer = OAuthIssuerIdentifier::new("http://pds.ripperoni.com").unwrap();
        let keyset = Keyset::new(vec![Box::new(jose_key)]);
        let keyset = Arc::new(RwLock::new(keyset));

        let algorithms: Option<Vec<Algorithm>> = None;
        let search_kids: Option<Vec<String>> = None;
        let sign_header = JwtHeader::default();
        let payload = JwtPayload {
            iss: None,
            aud: None,
            sub: None,
            exp: None,
            nbf: None,
            iat: None,
            jti: None,
            htm: None,
            htu: None,
            ath: None,
            acr: None,
            azp: None,
            amr: None,
            cnf: None,
            client_id: None,
            scope: None,
            nonce: None,
            at_hash: None,
            c_hash: None,
            s_hash: None,
            auth_time: None,
            name: None,
            family_name: None,
            given_name: None,
            middle_name: None,
            nickname: None,
            preferred_username: None,
            gender: None,
            picture: None,
            profile: None,
            website: None,
            birthdate: None,
            zoneinfo: None,
            locale: None,
            updated_at: None,
            email: None,
            email_verified: None,
            phone_number: None,
            phone_number_verified: None,
            address: None,
            authorization_details: None,
            additional_claims: Default::default(),
        };

        let signer = Signer::new(issuer, keyset);
        let result = signer
            .sign(algorithms, search_kids, sign_header, payload)
            .await
            .unwrap();
        let expected = SignedJwt::new("".to_string()).unwrap();
        assert_eq!(result, expected)
    }

    #[tokio::test]
    async fn test_access_token() {
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
        let jose_key = JoseKey::from_jwk(jwk, None).await;
        let issuer = OAuthIssuerIdentifier::new("http://pds.ripperoni.com").unwrap();
        let keyset = Keyset::new(vec![Box::new(jose_key)]);
        let keyset = Arc::new(RwLock::new(keyset));
        let signer = Signer::new(issuer, keyset);
        let client = Client {
            id: OAuthClientId::new("client123".to_string()).unwrap(),
            metadata: OAuthClientMetadata {
                redirect_uris: vec![],
                ..Default::default()
            },
            jwks: Some(JWKSet { keys: vec![] }),
            info: Default::default(),
        };
        let parameters = OAuthAuthorizationRequestParameters {
            client_id: OAuthClientId::new("client123".to_string()).unwrap(),
            state: None,
            redirect_uri: None,
            scope: None,
            response_type: OAuthResponseType::Code,
            code_challenge: None,
            code_challenge_method: None,
            dpop_jkt: None,
            response_mode: None,
            nonce: None,
            max_age: None,
            claims: None,
            login_hint: None,
            ui_locales: None,
            id_token_hint: None,
            display: None,
            prompt: None,
            authorization_details: None,
        };
        let options = AccessTokenOptions {
            aud: Audience::Single("did:web:pds.ripperoni.com".to_string()),
            sub: Sub::new("did:plc:wdadad".to_string()).unwrap(),
            jti: TokenId::new("tok-739361c165c76408088de74ee136cf66".to_string()).unwrap(),
            exp: Utc::now(),
            iat: None,
            alg: Some(Algorithm::Signature(jwa::SignatureAlgorithm::RS256)),
            cnf: None,
            authorization_details: None,
        };
        let result = signer
            .access_token(client, parameters, options)
            .await
            .unwrap();
        let expected = SignedJwt::new("").unwrap();
        assert_eq!(result, expected)
    }

    #[tokio::test]
    async fn test_verify_access_token() {
        let secret = Secret::rsa_keypair_from_file("/rsa_keypair").unwrap();
        let jose_key = JoseKey::from_secret(secret, None, None).await;
        let issuer = OAuthIssuerIdentifier::new("http://pds.ripperoni.com").unwrap();
        let keyset = Keyset::new(vec![Box::new(jose_key)]);
        let keyset = Arc::new(RwLock::new(keyset));

        let token = SignedJwt::new("eyJ0eXAiOiJKV1QiLCJhbGciOiJSUzI1NiIsImtpZCI6Ik5FTXlNRUZDTXpVd01URTFRVE5CT1VGRE1FUTFPRGN6UmprNU56QkdRelk0UVRrMVEwWkVPUSJ9.eyJpc3MiOiJodHRwczovL2Rldi1lanRsOTg4dy5hdXRoMC5jb20vIiwic3ViIjoiZ1pTeXNwQ1k1ZEk0aDFaM3Fwd3BkYjlUNFVQZEdENWtAY2xpZW50cyIsImF1ZCI6Imh0dHA6Ly9oZWxsb3dvcmxkIiwiaWF0IjoxNTcyNDA2NDQ3LCJleHAiOjE1NzI0OTI4NDcsImF6cCI6ImdaU3lzcENZNWRJNGgxWjNxcHdwZGI5VDRVUGRHRDVrIiwiZ3R5IjoiY2xpZW50LWNyZWRlbnRpYWxzIn0.nupgm7iFqSnERq9GxszwBrsYrYfMuSfUGj8tGQlkY3Ksh3o_IDfq1GO5ngHQLZuYPD-8qPIovPBEVomGZCo_jYvsbjmYkalAStmF01TvSoXQgJd09ygZstH0liKsmINStiRE8fTA-yfEIuBYttROizx-cDoxiindbKNIGOsqf6yOxf7ww8DrTBJKYRnHVkAfIK8wm9LRpsaOVzWdC7S3cbhCKvANjT0RTRpAx8b_AOr_UCpOr8paj-xMT9Zc9HVCMZLBfj6OZ6yVvnC9g6q_SlTa--fY9SL5eqy6-q1JGoyK_-BQ_YrCwrRdrjoJsJ8j-XFRFWJX09W3oDuZ990nGA").unwrap();

        let signer = Signer::new(issuer, keyset);
        let verify_options = VerifyOptions {
            audience: Some("http://helloworld".to_string()),
            clock_tolerance: None,
            issuer: None,
            max_token_age: None,
            subject: None,
            typ: None,
            current_date: None,
            required_claims: vec![],
        };
        let result = signer
            .verify_access_token(token, Some(verify_options))
            .await
            .unwrap();
        let expected = VerifyAccessTokenResponse {
            protected_header: Default::default(),
            payload: SignedTokenPayload {
                iat: 0,
                iss: "".to_string(),
                aud: Audience::Single("did:web:pds.ripperoni.com".to_string()),
                exp: 0,
                jti: TokenId::new("").unwrap(),
                sub: Sub::new("did:plc:khvyd3oiw46vif5gm7hijslk").unwrap(),
                client_id: OAuthClientId::new("".to_string()).unwrap(),
                nbf: None,
                htm: None,
                htu: None,
                ath: None,
                acr: None,
                azp: None,
                amr: None,
            },
        };
        assert_eq!(result, expected)
    }
}
