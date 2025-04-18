use crate::jwk::{
    algorithm_as_string, Audience, JwkError, JwtConfirmation, JwtHeader, JwtPayload, Key, Keyset,
    SignedJwt, VerifyOptions, VerifyResult,
};
use crate::oauth_provider::client::client::Client;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::oidc::sub::Sub;
use crate::oauth_provider::signer::signed_token_payload::SignedTokenPayload;
use crate::oauth_provider::token::token_id::TokenId;
use crate::oauth_types::{
    OAuthAuthorizationDetails, OAuthAuthorizationRequestParameters, OAuthIssuerIdentifier,
};
use biscuit::jwa::SignatureAlgorithm;
use rocket::form::FromForm;
use rocket::yansi::Paint;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct JwtSignHeader {}

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
                verify_options.issuer = Some(self.issuer.clone().into_inner());
                Some(verify_options)
            }
        };
        let result = keyset.verify_jwt(signed_jwt, verify_options).await;
        result
    }

    pub async fn sign(
        &self,
        algorithms: Option<Vec<SignatureAlgorithm>>,
        search_kids: Option<Vec<String>>,
        sign_header: JwtHeader,
        payload: JwtPayload,
    ) -> Result<SignedJwt, JwkError> {
        let mut keyset = self.keyset.write().await;
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
        let header = JwtHeader {
            alg: Some(algorithm_as_string(options.alg.unwrap())),
            typ: Some("at+jwt".to_string()),
            ..Default::default()
        };

        let mut payload = JwtPayload::default();
        payload.aud = Some(options.aud);
        payload.iat = options.iat;
        payload.exp = Some(options.exp);
        payload.sub = Some(options.sub);
        payload.jti = Some(options.jti);
        payload.cnf = options.cnf;
        // // https://datatracker.ietf.org/doc/html/rfc8693#section-4.3
        payload.client_id = Some(client.id);
        payload.scope = parameters.scope;
        payload.authorization_details = options.authorization_details;

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
            None => VerifyOptions {
                audience: None,
                clock_tolerance: None,
                issuer: None,
                max_token_age: None,
                subject: None,
                typ: None,
                current_date: None,
                required_claims: vec![],
            },
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
    pub exp: u64,
    pub iat: Option<u64>,
    pub alg: Option<SignatureAlgorithm>,
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
    //
    // #[tokio::test]
    // async fn test_verify() {
    //     let jwk = Jwk {
    //         common: CommonParameters {
    //             public_key_use: Some(PublicKeyUse::Signature),
    //             key_operations: None,
    //             key_algorithm: Some(KeyAlgorithm::RS256),
    //             key_id: Some("NEMyMEFCMzUwMTE1QTNBOUFDMEQ1ODczRjk5NzBGQzY4QTk1Q0ZEOQ".to_string()),
    //             x509_url: None,
    //             x509_chain: Some(vec!["MIIDBzCCAe+gAwIBAgIJakoPho0MJr56MA0GCSqGSIb3DQEBCwUAMCExHzAdBgNVBAMTFmRldi1lanRsOTg4dy5hdXRoMC5jb20wHhcNMTkxMDI5MjIwNzIyWhcNMzMwNzA3MjIwNzIyWjAhMR8wHQYDVQQDExZkZXYtZWp0bDk4OHcuYXV0aDAuY29tMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAzkM1QHcP0v8bmwQ2fd3Pj6unCTx5k8LsW9cuLtUhAjjzRGpSEwGCKEgi1ej2+0Cxcs1t0wzhO+zSv1TJbsDI0x862PIFEs3xkGqPZU6rfQMzvCmncAcMjuW7r/Zewm0s58oRGyic1Oyp8xiy78czlBG03jk/+/vdttJkie8pUc9AHBuMxAaV4iPN3zSi/J5OVSlovk607H3AUiL3Bfg4ssS1bsJvaFG0kuNscoiP+qLRTjFK6LzZS99VxegeNzttqGbtj5BwNgbtuzrIyfLmYB/9VgEw+QdaQHvxoAvD0f7aYsaJ1R6rrqxo+1Pun7j1/h7kOCGB0UcHDLDw7gaP/wIDAQABo0IwQDAPBgNVHRMBAf8EBTADAQH/MB0GA1UdDgQWBBQwIoo6QzzUL/TcNVpLGrLdd3DAIzAOBgNVHQ8BAf8EBAMCAoQwDQYJKoZIhvcNAQELBQADggEBALb8QycRmauyC/HRWRxTbl0w231HTAVYizQqhFQFl3beSQIhexGik+H+B4ve2rv94QRD3LlraUp+J26wLG89EnSCuCo/OxPAq+lxO6hNf6oKJ+Y2f48awIOxolO0f89qX3KMIkABXwKbYUcd+SBHX5ZP1V9cvJEyH0s3Fq9ObysPCH2j2Hjgz3WMIffSFMaO0DIfh3eNnv9hKQwavUO7fL/jqhBl4QxI2gMySi0Ni7PgAlBgxBx6YUp59q/lzMgAf19GOEOvI7l4dA0bc9pdsm7OhimskvOUSZYi5Pz3n/i/cTVKKhlj6NyINkMXlXGgyM9vEBpdcIpOWn/1H5QVy8Q=".to_string()]),
    //             x509_sha1_fingerprint: Some("NEMyMEFCMzUwMTE1QTNBOUFDMEQ1ODczRjk5NzBGQzY4QTk1Q0ZEOQ".to_string()),
    //             x509_sha256_fingerprint: None,
    //         },
    //         algorithm: AlgorithmParameters::RSA(RSAKeyParameters {
    //             key_type: Default::default(),
    //             n: "zkM1QHcP0v8bmwQ2fd3Pj6unCTx5k8LsW9cuLtUhAjjzRGpSEwGCKEgi1ej2-0Cxcs1t0wzhO-zSv1TJbsDI0x862PIFEs3xkGqPZU6rfQMzvCmncAcMjuW7r_Zewm0s58oRGyic1Oyp8xiy78czlBG03jk_-_vdttJkie8pUc9AHBuMxAaV4iPN3zSi_J5OVSlovk607H3AUiL3Bfg4ssS1bsJvaFG0kuNscoiP-qLRTjFK6LzZS99VxegeNzttqGbtj5BwNgbtuzrIyfLmYB_9VgEw-QdaQHvxoAvD0f7aYsaJ1R6rrqxo-1Pun7j1_h7kOCGB0UcHDLDw7gaP_w".to_string(),
    //             e: "AQAB".to_string(),
    //         }),
    //     };
    //     let jose_key = JoseKey::from_jwk(jwk, None).await;
    //     let issuer = OAuthIssuerIdentifier::new("http://pds.ripperoni.com").unwrap();
    //     let keyset = Keyset::new(vec![Box::new(jose_key)]);
    //     let keyset = Arc::new(RwLock::new(keyset));
    //
    //     let token = SignedJwt::new("eyJ0eXAiOiJKV1QiLCJhbGciOiJSUzI1NiIsImtpZCI6Ik5FTXlNRUZDTXpVd01URTFRVE5CT1VGRE1FUTFPRGN6UmprNU56QkdRelk0UVRrMVEwWkVPUSJ9.eyJpc3MiOiJodHRwczovL2Rldi1lanRsOTg4dy5hdXRoMC5jb20vIiwic3ViIjoiZ1pTeXNwQ1k1ZEk0aDFaM3Fwd3BkYjlUNFVQZEdENWtAY2xpZW50cyIsImF1ZCI6Imh0dHA6Ly9oZWxsb3dvcmxkIiwiaWF0IjoxNTcyNDA2NDQ3LCJleHAiOjE1NzI0OTI4NDcsImF6cCI6ImdaU3lzcENZNWRJNGgxWjNxcHdwZGI5VDRVUGRHRDVrIiwiZ3R5IjoiY2xpZW50LWNyZWRlbnRpYWxzIn0.nupgm7iFqSnERq9GxszwBrsYrYfMuSfUGj8tGQlkY3Ksh3o_IDfq1GO5ngHQLZuYPD-8qPIovPBEVomGZCo_jYvsbjmYkalAStmF01TvSoXQgJd09ygZstH0liKsmINStiRE8fTA-yfEIuBYttROizx-cDoxiindbKNIGOsqf6yOxf7ww8DrTBJKYRnHVkAfIK8wm9LRpsaOVzWdC7S3cbhCKvANjT0RTRpAx8b_AOr_UCpOr8paj-xMT9Zc9HVCMZLBfj6OZ6yVvnC9g6q_SlTa--fY9SL5eqy6-q1JGoyK_-BQ_YrCwrRdrjoJsJ8j-XFRFWJX09W3oDuZ990nGA").unwrap();
    //
    //     let signer = Signer::new(issuer, keyset);
    //     let mut validation = Validation::new(Algorithm::RS256);
    //     validation.leeway = 1572406447;
    //     let mut x = HashSet::new();
    //     x.insert("http://helloworld".to_string());
    //     validation.aud = Some(x);
    //     let result = signer.verify(token, Some(validation)).await.unwrap();
    //     let expected = VerifyResult {
    //         payload: Default::default(),
    //         protected_header: Default::default(),
    //     };
    //     assert_eq!(result, expected);
    // }
    //
    // #[tokio::test]
    // async fn test_sign() {
    //     let jwk = Jwk {
    //         common: CommonParameters {
    //             public_key_use: Some(PublicKeyUse::Signature),
    //             key_operations: None,
    //             key_algorithm: Some(KeyAlgorithm::RS256),
    //             key_id: Some("NEMyMEFCMzUwMTE1QTNBOUFDMEQ1ODczRjk5NzBGQzY4QTk1Q0ZEOQ".to_string()),
    //             x509_url: None,
    //             x509_chain: Some(vec!["MIIDBzCCAe+gAwIBAgIJakoPho0MJr56MA0GCSqGSIb3DQEBCwUAMCExHzAdBgNVBAMTFmRldi1lanRsOTg4dy5hdXRoMC5jb20wHhcNMTkxMDI5MjIwNzIyWhcNMzMwNzA3MjIwNzIyWjAhMR8wHQYDVQQDExZkZXYtZWp0bDk4OHcuYXV0aDAuY29tMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAzkM1QHcP0v8bmwQ2fd3Pj6unCTx5k8LsW9cuLtUhAjjzRGpSEwGCKEgi1ej2+0Cxcs1t0wzhO+zSv1TJbsDI0x862PIFEs3xkGqPZU6rfQMzvCmncAcMjuW7r/Zewm0s58oRGyic1Oyp8xiy78czlBG03jk/+/vdttJkie8pUc9AHBuMxAaV4iPN3zSi/J5OVSlovk607H3AUiL3Bfg4ssS1bsJvaFG0kuNscoiP+qLRTjFK6LzZS99VxegeNzttqGbtj5BwNgbtuzrIyfLmYB/9VgEw+QdaQHvxoAvD0f7aYsaJ1R6rrqxo+1Pun7j1/h7kOCGB0UcHDLDw7gaP/wIDAQABo0IwQDAPBgNVHRMBAf8EBTADAQH/MB0GA1UdDgQWBBQwIoo6QzzUL/TcNVpLGrLdd3DAIzAOBgNVHQ8BAf8EBAMCAoQwDQYJKoZIhvcNAQELBQADggEBALb8QycRmauyC/HRWRxTbl0w231HTAVYizQqhFQFl3beSQIhexGik+H+B4ve2rv94QRD3LlraUp+J26wLG89EnSCuCo/OxPAq+lxO6hNf6oKJ+Y2f48awIOxolO0f89qX3KMIkABXwKbYUcd+SBHX5ZP1V9cvJEyH0s3Fq9ObysPCH2j2Hjgz3WMIffSFMaO0DIfh3eNnv9hKQwavUO7fL/jqhBl4QxI2gMySi0Ni7PgAlBgxBx6YUp59q/lzMgAf19GOEOvI7l4dA0bc9pdsm7OhimskvOUSZYi5Pz3n/i/cTVKKhlj6NyINkMXlXGgyM9vEBpdcIpOWn/1H5QVy8Q=".to_string()]),
    //             x509_sha1_fingerprint: Some("NEMyMEFCMzUwMTE1QTNBOUFDMEQ1ODczRjk5NzBGQzY4QTk1Q0ZEOQ".to_string()),
    //             x509_sha256_fingerprint: None,
    //         },
    //         algorithm: AlgorithmParameters::RSA(RSAKeyParameters {
    //             key_type: Default::default(),
    //             n: "zkM1QHcP0v8bmwQ2fd3Pj6unCTx5k8LsW9cuLtUhAjjzRGpSEwGCKEgi1ej2-0Cxcs1t0wzhO-zSv1TJbsDI0x862PIFEs3xkGqPZU6rfQMzvCmncAcMjuW7r_Zewm0s58oRGyic1Oyp8xiy78czlBG03jk_-_vdttJkie8pUc9AHBuMxAaV4iPN3zSi_J5OVSlovk607H3AUiL3Bfg4ssS1bsJvaFG0kuNscoiP-qLRTjFK6LzZS99VxegeNzttqGbtj5BwNgbtuzrIyfLmYB_9VgEw-QdaQHvxoAvD0f7aYsaJ1R6rrqxo-1Pun7j1_h7kOCGB0UcHDLDw7gaP_w".to_string(),
    //             e: "AQAB".to_string(),
    //         }),
    //     };
    //     let jose_key = JoseKey::from_jwk(jwk, None).await;
    //     let issuer = OAuthIssuerIdentifier::new("http://pds.ripperoni.com").unwrap();
    //     let keyset = Keyset::new(vec![Box::new(jose_key)]);
    //     let keyset = Arc::new(RwLock::new(keyset));
    //
    //     let algorithms: Option<Vec<Algorithm>> = None;
    //     let search_kids: Option<Vec<String>> = None;
    //     let sign_header = Header::default();
    //     let payload = JwtPayload {
    //         iss: None,
    //         aud: None,
    //         sub: None,
    //         exp: None,
    //         nbf: None,
    //         iat: None,
    //         jti: None,
    //         htm: None,
    //         htu: None,
    //         ath: None,
    //         acr: None,
    //         azp: None,
    //         amr: None,
    //         cnf: None,
    //         client_id: None,
    //         scope: None,
    //         nonce: None,
    //         at_hash: None,
    //         c_hash: None,
    //         s_hash: None,
    //         auth_time: None,
    //         name: None,
    //         family_name: None,
    //         given_name: None,
    //         middle_name: None,
    //         nickname: None,
    //         preferred_username: None,
    //         gender: None,
    //         picture: None,
    //         profile: None,
    //         website: None,
    //         birthdate: None,
    //         zoneinfo: None,
    //         locale: None,
    //         updated_at: None,
    //         email: None,
    //         email_verified: None,
    //         phone_number: None,
    //         phone_number_verified: None,
    //         address: None,
    //         authorization_details: None,
    //         additional_claims: Default::default(),
    //     };
    //
    //     let header = Header::default();
    //     let signer = Signer::new(issuer, keyset);
    //     let result = signer
    //         .sign(algorithms, search_kids, sign_header, payload)
    //         .await
    //         .unwrap();
    //     let expected = SignedJwt::new("".to_string()).unwrap();
    //     assert_eq!(result, expected)
    // }
    //
    // #[tokio::test]
    // async fn test_access_token() {
    //     let jwk = Jwk {
    //         common: CommonParameters {
    //             public_key_use: Some(PublicKeyUse::Signature),
    //             key_operations: None,
    //             key_algorithm: Some(KeyAlgorithm::RS256),
    //             key_id: Some("NEMyMEFCMzUwMTE1QTNBOUFDMEQ1ODczRjk5NzBGQzY4QTk1Q0ZEOQ".to_string()),
    //             x509_url: None,
    //             x509_chain: Some(vec!["MIIDBzCCAe+gAwIBAgIJakoPho0MJr56MA0GCSqGSIb3DQEBCwUAMCExHzAdBgNVBAMTFmRldi1lanRsOTg4dy5hdXRoMC5jb20wHhcNMTkxMDI5MjIwNzIyWhcNMzMwNzA3MjIwNzIyWjAhMR8wHQYDVQQDExZkZXYtZWp0bDk4OHcuYXV0aDAuY29tMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAzkM1QHcP0v8bmwQ2fd3Pj6unCTx5k8LsW9cuLtUhAjjzRGpSEwGCKEgi1ej2+0Cxcs1t0wzhO+zSv1TJbsDI0x862PIFEs3xkGqPZU6rfQMzvCmncAcMjuW7r/Zewm0s58oRGyic1Oyp8xiy78czlBG03jk/+/vdttJkie8pUc9AHBuMxAaV4iPN3zSi/J5OVSlovk607H3AUiL3Bfg4ssS1bsJvaFG0kuNscoiP+qLRTjFK6LzZS99VxegeNzttqGbtj5BwNgbtuzrIyfLmYB/9VgEw+QdaQHvxoAvD0f7aYsaJ1R6rrqxo+1Pun7j1/h7kOCGB0UcHDLDw7gaP/wIDAQABo0IwQDAPBgNVHRMBAf8EBTADAQH/MB0GA1UdDgQWBBQwIoo6QzzUL/TcNVpLGrLdd3DAIzAOBgNVHQ8BAf8EBAMCAoQwDQYJKoZIhvcNAQELBQADggEBALb8QycRmauyC/HRWRxTbl0w231HTAVYizQqhFQFl3beSQIhexGik+H+B4ve2rv94QRD3LlraUp+J26wLG89EnSCuCo/OxPAq+lxO6hNf6oKJ+Y2f48awIOxolO0f89qX3KMIkABXwKbYUcd+SBHX5ZP1V9cvJEyH0s3Fq9ObysPCH2j2Hjgz3WMIffSFMaO0DIfh3eNnv9hKQwavUO7fL/jqhBl4QxI2gMySi0Ni7PgAlBgxBx6YUp59q/lzMgAf19GOEOvI7l4dA0bc9pdsm7OhimskvOUSZYi5Pz3n/i/cTVKKhlj6NyINkMXlXGgyM9vEBpdcIpOWn/1H5QVy8Q=".to_string()]),
    //             x509_sha1_fingerprint: Some("NEMyMEFCMzUwMTE1QTNBOUFDMEQ1ODczRjk5NzBGQzY4QTk1Q0ZEOQ".to_string()),
    //             x509_sha256_fingerprint: None,
    //         },
    //         algorithm: AlgorithmParameters::RSA(RSAKeyParameters {
    //             key_type: Default::default(),
    //             n: "zkM1QHcP0v8bmwQ2fd3Pj6unCTx5k8LsW9cuLtUhAjjzRGpSEwGCKEgi1ej2-0Cxcs1t0wzhO-zSv1TJbsDI0x862PIFEs3xkGqPZU6rfQMzvCmncAcMjuW7r_Zewm0s58oRGyic1Oyp8xiy78czlBG03jk_-_vdttJkie8pUc9AHBuMxAaV4iPN3zSi_J5OVSlovk607H3AUiL3Bfg4ssS1bsJvaFG0kuNscoiP-qLRTjFK6LzZS99VxegeNzttqGbtj5BwNgbtuzrIyfLmYB_9VgEw-QdaQHvxoAvD0f7aYsaJ1R6rrqxo-1Pun7j1_h7kOCGB0UcHDLDw7gaP_w".to_string(),
    //             e: "AQAB".to_string(),
    //         }),
    //     };
    //     let jose_key = JoseKey::from_jwk(jwk, None).await;
    //     let issuer = OAuthIssuerIdentifier::new("http://pds.ripperoni.com").unwrap();
    //     let keyset = Keyset::new(vec![Box::new(jose_key)]);
    //     let keyset = Arc::new(RwLock::new(keyset));
    //     let signer = Signer::new(issuer, keyset);
    //     let client = Client {
    //         id: OAuthClientId::new("client123".to_string()).unwrap(),
    //         metadata: OAuthClientMetadata {
    //             redirect_uris: vec![],
    //             response_types: vec![],
    //             grant_types: vec![],
    //             scope: None,
    //             token_endpoint_auth_method: None,
    //             token_endpoint_auth_signing_alg: None,
    //             userinfo_signed_response_alg: None,
    //             userinfo_encrypted_response_alg: None,
    //             jwks_uri: None,
    //             jwks: None,
    //             application_type: Default::default(),
    //             subject_type: None,
    //             request_object_signing_alg: None,
    //             id_token_signed_response_alg: None,
    //             authorization_signed_response_alg: "".to_string(),
    //             authorization_encrypted_response_enc: None,
    //             authorization_encrypted_response_alg: None,
    //             client_id: None,
    //             client_name: None,
    //             client_uri: None,
    //             policy_uri: None,
    //             tos_uri: None,
    //             logo_uri: None,
    //             default_max_age: None,
    //             require_auth_time: None,
    //             contacts: None,
    //             tls_client_certificate_bound_access_tokens: None,
    //             dpop_bound_access_tokens: None,
    //             authorization_details_types: None,
    //         },
    //         jwks: None,
    //         info: Default::default(),
    //     };
    //     let parameters = OAuthAuthorizationRequestParameters {
    //         client_id: OAuthClientId::new(
    //             "https://cleanfollow-bsky.pages.dev/client-metadata.json".to_string(),
    //         )
    //         .unwrap(),
    //         state: None,
    //         redirect_uri: None,
    //         scope: None,
    //         response_type: OAuthResponseType::Code,
    //         code_challenge: None,
    //         code_challenge_method: None,
    //         dpop_jkt: None,
    //         response_mode: None,
    //         nonce: None,
    //         max_age: None,
    //         claims: None,
    //         login_hint: None,
    //         ui_locales: None,
    //         id_token_hint: None,
    //         display: None,
    //         prompt: None,
    //         authorization_details: None,
    //     };
    //     let options = AccessTokenOptions {
    //         aud: Audience::Single("".to_string()),
    //         sub: Sub::new("did:plc:khvyd3oiw46vif5gm7hijslk".to_string()).unwrap(),
    //         jti: TokenId::new("".to_string()).unwrap(),
    //         exp: 0,
    //         iat: None,
    //         alg: None,
    //         cnf: None,
    //         authorization_details: None,
    //     };
    //     let result = signer
    //         .access_token(client, parameters, options)
    //         .await
    //         .unwrap();
    //     let expected = SignedJwt::new("").unwrap();
    //     assert_eq!(result, expected)
    // }
    //
    // #[tokio::test]
    // async fn test_verify_access_token() {
    //     let jwk = Jwk {
    //         common: CommonParameters {
    //             public_key_use: Some(PublicKeyUse::Signature),
    //             key_operations: None,
    //             key_algorithm: Some(KeyAlgorithm::RS256),
    //             key_id: Some("NEMyMEFCMzUwMTE1QTNBOUFDMEQ1ODczRjk5NzBGQzY4QTk1Q0ZEOQ".to_string()),
    //             x509_url: None,
    //             x509_chain: Some(vec!["MIIDBzCCAe+gAwIBAgIJakoPho0MJr56MA0GCSqGSIb3DQEBCwUAMCExHzAdBgNVBAMTFmRldi1lanRsOTg4dy5hdXRoMC5jb20wHhcNMTkxMDI5MjIwNzIyWhcNMzMwNzA3MjIwNzIyWjAhMR8wHQYDVQQDExZkZXYtZWp0bDk4OHcuYXV0aDAuY29tMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAzkM1QHcP0v8bmwQ2fd3Pj6unCTx5k8LsW9cuLtUhAjjzRGpSEwGCKEgi1ej2+0Cxcs1t0wzhO+zSv1TJbsDI0x862PIFEs3xkGqPZU6rfQMzvCmncAcMjuW7r/Zewm0s58oRGyic1Oyp8xiy78czlBG03jk/+/vdttJkie8pUc9AHBuMxAaV4iPN3zSi/J5OVSlovk607H3AUiL3Bfg4ssS1bsJvaFG0kuNscoiP+qLRTjFK6LzZS99VxegeNzttqGbtj5BwNgbtuzrIyfLmYB/9VgEw+QdaQHvxoAvD0f7aYsaJ1R6rrqxo+1Pun7j1/h7kOCGB0UcHDLDw7gaP/wIDAQABo0IwQDAPBgNVHRMBAf8EBTADAQH/MB0GA1UdDgQWBBQwIoo6QzzUL/TcNVpLGrLdd3DAIzAOBgNVHQ8BAf8EBAMCAoQwDQYJKoZIhvcNAQELBQADggEBALb8QycRmauyC/HRWRxTbl0w231HTAVYizQqhFQFl3beSQIhexGik+H+B4ve2rv94QRD3LlraUp+J26wLG89EnSCuCo/OxPAq+lxO6hNf6oKJ+Y2f48awIOxolO0f89qX3KMIkABXwKbYUcd+SBHX5ZP1V9cvJEyH0s3Fq9ObysPCH2j2Hjgz3WMIffSFMaO0DIfh3eNnv9hKQwavUO7fL/jqhBl4QxI2gMySi0Ni7PgAlBgxBx6YUp59q/lzMgAf19GOEOvI7l4dA0bc9pdsm7OhimskvOUSZYi5Pz3n/i/cTVKKhlj6NyINkMXlXGgyM9vEBpdcIpOWn/1H5QVy8Q=".to_string()]),
    //             x509_sha1_fingerprint: Some("NEMyMEFCMzUwMTE1QTNBOUFDMEQ1ODczRjk5NzBGQzY4QTk1Q0ZEOQ".to_string()),
    //             x509_sha256_fingerprint: None,
    //         },
    //         algorithm: AlgorithmParameters::RSA(RSAKeyParameters {
    //             key_type: Default::default(),
    //             n: "zkM1QHcP0v8bmwQ2fd3Pj6unCTx5k8LsW9cuLtUhAjjzRGpSEwGCKEgi1ej2-0Cxcs1t0wzhO-zSv1TJbsDI0x862PIFEs3xkGqPZU6rfQMzvCmncAcMjuW7r_Zewm0s58oRGyic1Oyp8xiy78czlBG03jk_-_vdttJkie8pUc9AHBuMxAaV4iPN3zSi_J5OVSlovk607H3AUiL3Bfg4ssS1bsJvaFG0kuNscoiP-qLRTjFK6LzZS99VxegeNzttqGbtj5BwNgbtuzrIyfLmYB_9VgEw-QdaQHvxoAvD0f7aYsaJ1R6rrqxo-1Pun7j1_h7kOCGB0UcHDLDw7gaP_w".to_string(),
    //             e: "AQAB".to_string(),
    //         }),
    //     };
    //     let jose_key = JoseKey::from_jwk(jwk, None).await;
    //     let issuer = OAuthIssuerIdentifier::new("http://pds.ripperoni.com").unwrap();
    //     let keyset = Keyset::new(vec![Box::new(jose_key)]);
    //     let keyset = Arc::new(RwLock::new(keyset));
    //
    //     let token = SignedJwt::new("eyJ0eXAiOiJKV1QiLCJhbGciOiJSUzI1NiIsImtpZCI6Ik5FTXlNRUZDTXpVd01URTFRVE5CT1VGRE1FUTFPRGN6UmprNU56QkdRelk0UVRrMVEwWkVPUSJ9.eyJpc3MiOiJodHRwczovL2Rldi1lanRsOTg4dy5hdXRoMC5jb20vIiwic3ViIjoiZ1pTeXNwQ1k1ZEk0aDFaM3Fwd3BkYjlUNFVQZEdENWtAY2xpZW50cyIsImF1ZCI6Imh0dHA6Ly9oZWxsb3dvcmxkIiwiaWF0IjoxNTcyNDA2NDQ3LCJleHAiOjE1NzI0OTI4NDcsImF6cCI6ImdaU3lzcENZNWRJNGgxWjNxcHdwZGI5VDRVUGRHRDVrIiwiZ3R5IjoiY2xpZW50LWNyZWRlbnRpYWxzIn0.nupgm7iFqSnERq9GxszwBrsYrYfMuSfUGj8tGQlkY3Ksh3o_IDfq1GO5ngHQLZuYPD-8qPIovPBEVomGZCo_jYvsbjmYkalAStmF01TvSoXQgJd09ygZstH0liKsmINStiRE8fTA-yfEIuBYttROizx-cDoxiindbKNIGOsqf6yOxf7ww8DrTBJKYRnHVkAfIK8wm9LRpsaOVzWdC7S3cbhCKvANjT0RTRpAx8b_AOr_UCpOr8paj-xMT9Zc9HVCMZLBfj6OZ6yVvnC9g6q_SlTa--fY9SL5eqy6-q1JGoyK_-BQ_YrCwrRdrjoJsJ8j-XFRFWJX09W3oDuZ990nGA").unwrap();
    //
    //     let signer = Signer::new(issuer, keyset);
    //     let mut validation = Validation::new(Algorithm::RS256);
    //     validation.leeway = 1572406447;
    //     let mut x = HashSet::new();
    //     x.insert("http://helloworld".to_string());
    //     validation.aud = Some(x);
    //     let result = signer
    //         .verify_access_token(token, Some(validation))
    //         .await
    //         .unwrap();
    //     let expected = VerifyAccessTokenResponse {
    //         protected_header: Default::default(),
    //         payload: SignedTokenPayload {
    //             iat: 0,
    //             iss: "".to_string(),
    //             aud: Audience::Single("".to_string()),
    //             exp: 0,
    //             jti: TokenId::new("").unwrap(),
    //             sub: Sub::new("did:plc:khvyd3oiw46vif5gm7hijslk").unwrap(),
    //             client_id: OAuthClientId::new(
    //                 "https://cleanfollow-bsky.pages.dev/client-metadata.json".to_string(),
    //             )
    //             .unwrap(),
    //             nbf: None,
    //             htm: None,
    //             htu: None,
    //             ath: None,
    //             acr: None,
    //             azp: None,
    //             amr: None,
    //         },
    //     };
    //     assert_eq!(result, expected)
    // }
}
