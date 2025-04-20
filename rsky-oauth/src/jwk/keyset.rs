use crate::jwk::key::Key;
use crate::jwk::{JwkError, JwtPayload, SignedJwt, VerifyOptions, VerifyResult};
use jsonwebtoken::jwk::{Jwk, PublicKeyUse};
use jsonwebtoken::{Algorithm, Header};
use rocket::form::validate::Contains;
use std::collections::HashSet;

pub struct Keyset {
    keys: Vec<Box<dyn Key>>,
    preferred_signing_algorithms: Vec<Algorithm>,
}

#[derive(Clone)]
pub struct KeySearch {
    pub r#use: Option<PublicKeyUse>,
    pub kid: Option<Vec<String>>,
    pub alg: Option<Vec<Algorithm>>,
}

impl Keyset {
    /**
     * The preferred algorithms to use when signing a JWT using this keyset.
     *
     * @see {@link https://datatracker.ietf.org/doc/html/rfc7518#section-3.1}
     */
    pub fn new(keys: Vec<Box<dyn Key>>) -> Self {
        Self {
            keys,
            preferred_signing_algorithms: vec![
                // Prefer elliptic curve algorithms
                Algorithm::EdDSA,
                Algorithm::ES256,
                // https://datatracker.ietf.org/doc/html/rfc7518#section-3.5
                Algorithm::PS256,
                Algorithm::PS384,
                Algorithm::PS512,
                Algorithm::HS256,
                Algorithm::HS384,
                Algorithm::HS512,
            ],
        }
    }

    pub fn size(&self) -> usize {
        self.keys.len()
    }

    pub fn sign_algorithms(&self) -> Vec<Algorithm> {
        let mut algorithms = HashSet::new();
        for key in &self.keys {
            if let Some(r#use) = key.r#use() {
                if r#use == PublicKeyUse::Signature {
                    for alg in key.algorithms() {
                        algorithms.insert(alg);
                    }
                }
            }
        }
        Vec::from_iter(algorithms)
    }

    pub fn public_jwks(&self) -> Vec<Jwk> {
        let mut result = vec![];
        for key in &self.keys {
            if let Some(jwk) = key.public_jwk() {
                result.push(jwk);
            }
        }
        result
    }

    pub fn private_jwks(&self) -> Vec<Jwk> {
        let mut result = vec![];
        for key in &self.keys {
            if let Some(jwk) = key.private_jwk() {
                result.push(jwk);
            }
        }
        result
    }

    pub fn has(&self, kid: &str) -> bool {
        for key in &self.keys {
            if let Some(key_kid) = key.kid() {
                if key_kid == kid {
                    return true;
                }
            }
        }
        false
    }

    async fn get(&self, search: KeySearch) {
        if let Some(key) = (self.list(search).await).into_iter().next() {
            unimplemented!()
        }
        unimplemented!()
    }

    async fn list(&self, search: KeySearch) -> Vec<&Box<dyn Key>> {
        let mut result = vec![];
        for key in &self.keys {
            if let Some(search_use) = &search.r#use {
                if let Some(key_use) = key.r#use() {
                    if search_use != &key_use {
                        continue;
                    }
                }
            }

            if let Some(search_kid) = &search.kid {
                if let Some(key_kid) = &key.kid() {
                    if !search_kid.contains(key_kid) {
                        continue;
                    }
                } else {
                    continue;
                }
            }

            if let Some(search_algs) = &search.alg {
                let key_algorithms = key.algorithms();
                for search_alg in search_algs {
                    if !key_algorithms.contains(search_alg) {
                        continue;
                    }
                }
            }

            result.push(key);
        }

        result
    }

    async fn find_key(
        &self,
        key_search: KeySearch,
    ) -> Result<(&Box<dyn Key>, Algorithm), JwkError> {
        let mut matching_keys: Vec<&Box<dyn Key>> = vec![];
        for key in self.list(key_search.clone()).await {
            // Not a signing key
            if !key.is_private() {
                continue;
            }

            // Skip negotiation if a specific "alg" was provided
            if let Some(search_algs) = &key_search.alg {
                if search_algs.len() == 1 {
                    return Ok((key, search_algs.get(0).unwrap().clone()));
                }
            }

            matching_keys.push(key);
        }

        let candidates: Vec<(&Box<dyn Key>, Vec<Algorithm>)> = matching_keys
            .into_iter()
            .map(|matching_key| (matching_key.clone(), matching_key.algorithms()))
            .collect();

        // Return the first candidates that matches the preferred algorithms
        for pref_alg in &self.preferred_signing_algorithms {
            for (matching_key, matching_algs) in &candidates {
                if matching_algs.contains(pref_alg) {
                    return Ok((matching_key, pref_alg.clone()));
                }
            }
        }

        // Return any candidate
        for (matching_key, matching_algs) in candidates {
            if let Some(alg) = matching_algs.into_iter().next() {
                return Ok((&matching_key, alg));
            }
        }

        Err(JwkError::Other("No signing key found".to_string()))
    }

    pub async fn create_jwt(
        &self,
        algorithms: Option<Vec<Algorithm>>,
        search_kids: Option<Vec<String>>,
        header: Header,
        jwt_payload: JwtPayload,
    ) -> Result<SignedJwt, JwkError> {
        let mut header = header.clone();
        let key_search = KeySearch {
            r#use: Some(PublicKeyUse::Signature),
            kid: search_kids,
            alg: algorithms,
        };
        let (key, alg) = match self.find_key(key_search).await {
            Ok(result) => result,
            Err(error) => return Err(JwkError::JwtCreateError("".to_string())),
        };
        header.alg = alg;
        header.kid = key.kid();
        key.create_jwt(header, jwt_payload).await
    }

    pub async fn verify_jwt(
        &self,
        signed_jwt: SignedJwt,
        options: Option<VerifyOptions>,
    ) -> Result<VerifyResult, JwkError> {
        let header = jsonwebtoken::decode_header(signed_jwt.val().as_str()).expect("No header");
        let kid = match header.kid {
            Some(kid) => kid,
            None => return Err(JwkError::JwtVerifyError("No kid supplied".to_string())),
        };

        let mut errors = vec![];

        let key_search = KeySearch {
            r#use: None,
            kid: Some(vec![kid]),
            alg: Some(vec![header.alg]),
        };

        for key in self.list(key_search).await {
            match key.verify_jwt(signed_jwt.clone(), options.clone()).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    errors.push(e);
                }
            }
        }

        if errors.is_empty() {
            Err(JwkError::JwtVerifyError(
                "ERR_JWKS_NO_MATCHING_KEY".to_string(),
            ))
        } else if errors.len() == 1 {
            Err(JwkError::JwtVerifyError("ERR_JWT_INVALID".to_string()))
        } else {
            Err(JwkError::JwtVerifyError("ERR_JWT_INVALID".to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::jwk::{Audience, JwtPayload, Key, Keyset, SignedJwt, VerifyOptions, VerifyResult};
    use crate::jwk_jose::jose_key::JoseKey;
    use crate::oauth_provider::oidc::sub::Sub;
    use jsonwebtoken::jwk::{
        AlgorithmParameters, CommonParameters, Jwk, KeyAlgorithm, PublicKeyUse, RSAKeyParameters,
    };
    use jsonwebtoken::{Algorithm, Header};

    #[tokio::test]
    async fn test_verify_jwt() {
        let mut keys: Vec<Box<dyn Key>> = vec![];
        let jwk = Jwk {
            common: CommonParameters {
                public_key_use: Some(PublicKeyUse::Signature),
                key_operations: None,
                key_algorithm: Some(KeyAlgorithm::RS256),
                key_id: Some("NEMyMEFCMzUwMTE1QTNBOUFDMEQ1ODczRjk5NzBGQzY4QTk1Q0ZEOQ".to_string()),
                x509_url: None,
                x509_chain: Some(vec!["MIIDBzCCAe+gAwIBAgIJakoPho0MJr56MA0GCSqGSIb3DQEBCwUAMCExHzAdBgNVBAMTFmRldi1lanRsOTg4dy5hdXRoMC5jb20wHhcNMTkxMDI5MjIwNzIyWhcNMzMwNzA3MjIwNzIyWjAhMR8wHQYDVQQDExZkZXYtZWp0bDk4OHcuYXV0aDAuY29tMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAzkM1QHcP0v8bmwQ2fd3Pj6unCTx5k8LsW9cuLtUhAjjzRGpSEwGCKEgi1ej2+0Cxcs1t0wzhO+zSv1TJbsDI0x862PIFEs3xkGqPZU6rfQMzvCmncAcMjuW7r/Zewm0s58oRGyic1Oyp8xiy78czlBG03jk/+/vdttJkie8pUc9AHBuMxAaV4iPN3zSi/J5OVSlovk607H3AUiL3Bfg4ssS1bsJvaFG0kuNscoiP+qLRTjFK6LzZS99VxegeNzttqGbtj5BwNgbtuzrIyfLmYB/9VgEw+QdaQHvxoAvD0f7aYsaJ1R6rrqxo+1Pun7j1/h7kOCGB0UcHDLDw7gaP/wIDAQABo0IwQDAPBgNVHRMBAf8EBTADAQH/MB0GA1UdDgQWBBQwIoo6QzzUL/TcNVpLGrLdd3DAIzAOBgNVHQ8BAf8EBAMCAoQwDQYJKoZIhvcNAQELBQADggEBALb8QycRmauyC/HRWRxTbl0w231HTAVYizQqhFQFl3beSQIhexGik+H+B4ve2rv94QRD3LlraUp+J26wLG89EnSCuCo/OxPAq+lxO6hNf6oKJ+Y2f48awIOxolO0f89qX3KMIkABXwKbYUcd+SBHX5ZP1V9cvJEyH0s3Fq9ObysPCH2j2Hjgz3WMIffSFMaO0DIfh3eNnv9hKQwavUO7fL/jqhBl4QxI2gMySi0Ni7PgAlBgxBx6YUp59q/lzMgAf19GOEOvI7l4dA0bc9pdsm7OhimskvOUSZYi5Pz3n/i/cTVKKhlj6NyINkMXlXGgyM9vEBpdcIpOWn/1H5QVy8Q=".to_string()]),
                x509_sha1_fingerprint: Some("NEMyMEFCMzUwMTE1QTNBOUFDMEQ1ODczRjk5NzBGQzY4QTk1Q0ZEOQ".to_string()),
                x509_sha256_fingerprint: None,
            },
            algorithm: AlgorithmParameters::RSA(RSAKeyParameters {
                key_type: Default::default(),
                n: "zkM1QHcP0v8bmwQ2fd3Pj6unCTx5k8LsW9cuLtUhAjjzRGpSEwGCKEgi1ej2-0Cxcs1t0wzhO-zSv1TJbsDI0x862PIFEs3xkGqPZU6rfQMzvCmncAcMjuW7r_Zewm0s58oRGyic1Oyp8xiy78czlBG03jk_-_vdttJkie8pUc9AHBuMxAaV4iPN3zSi_J5OVSlovk607H3AUiL3Bfg4ssS1bsJvaFG0kuNscoiP-qLRTjFK6LzZS99VxegeNzttqGbtj5BwNgbtuzrIyfLmYB_9VgEw-QdaQHvxoAvD0f7aYsaJ1R6rrqxo-1Pun7j1_h7kOCGB0UcHDLDw7gaP_w".to_string(),
                e: "AQAB".to_string(),
            }),
        };
        let jose_key = JoseKey::from_jwk(jwk, None).await;
        keys.push(Box::new(jose_key));
        let keyset = Keyset::new(keys);
        let token = SignedJwt::new("eyJ0eXAiOiJKV1QiLCJhbGciOiJSUzI1NiIsImtpZCI6Ik5FTXlNRUZDTXpVd01URTFRVE5CT1VGRE1FUTFPRGN6UmprNU56QkdRelk0UVRrMVEwWkVPUSJ9.eyJpc3MiOiJodHRwczovL2Rldi1lanRsOTg4dy5hdXRoMC5jb20vIiwic3ViIjoiZ1pTeXNwQ1k1ZEk0aDFaM3Fwd3BkYjlUNFVQZEdENWtAY2xpZW50cyIsImF1ZCI6Imh0dHA6Ly9oZWxsb3dvcmxkIiwiaWF0IjoxNTcyNDA2NDQ3LCJleHAiOjE1NzI0OTI4NDcsImF6cCI6ImdaU3lzcENZNWRJNGgxWjNxcHdwZGI5VDRVUGRHRDVrIiwiZ3R5IjoiY2xpZW50LWNyZWRlbnRpYWxzIn0.nupgm7iFqSnERq9GxszwBrsYrYfMuSfUGj8tGQlkY3Ksh3o_IDfq1GO5ngHQLZuYPD-8qPIovPBEVomGZCo_jYvsbjmYkalAStmF01TvSoXQgJd09ygZstH0liKsmINStiRE8fTA-yfEIuBYttROizx-cDoxiindbKNIGOsqf6yOxf7ww8DrTBJKYRnHVkAfIK8wm9LRpsaOVzWdC7S3cbhCKvANjT0RTRpAx8b_AOr_UCpOr8paj-xMT9Zc9HVCMZLBfj6OZ6yVvnC9g6q_SlTa--fY9SL5eqy6-q1JGoyK_-BQ_YrCwrRdrjoJsJ8j-XFRFWJX09W3oDuZ990nGA").unwrap();
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
        let result = keyset
            .verify_jwt(token, Some(verify_options))
            .await
            .unwrap();
        let expected = VerifyResult {
            payload: Default::default(),
            protected_header: Default::default(),
        };
        // assert_eq!(result, expected)
    }

    #[tokio::test]
    async fn test_create_jwt() {
        let mut keys: Vec<Box<dyn Key>> = vec![];
        let jwk = Jwk {
            common: CommonParameters {
                public_key_use: Some(PublicKeyUse::Signature),
                key_operations: None,
                key_algorithm: Some(KeyAlgorithm::RS256),
                key_id: Some("NEMyMEFCMzUwMTE1QTNBOUFDMEQ1ODczRjk5NzBGQzY4QTk1Q0ZEOQ".to_string()),
                x509_url: None,
                x509_chain: Some(vec!["MIIDBzCCAe+gAwIBAgIJakoPho0MJr56MA0GCSqGSIb3DQEBCwUAMCExHzAdBgNVBAMTFmRldi1lanRsOTg4dy5hdXRoMC5jb20wHhcNMTkxMDI5MjIwNzIyWhcNMzMwNzA3MjIwNzIyWjAhMR8wHQYDVQQDExZkZXYtZWp0bDk4OHcuYXV0aDAuY29tMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAzkM1QHcP0v8bmwQ2fd3Pj6unCTx5k8LsW9cuLtUhAjjzRGpSEwGCKEgi1ej2+0Cxcs1t0wzhO+zSv1TJbsDI0x862PIFEs3xkGqPZU6rfQMzvCmncAcMjuW7r/Zewm0s58oRGyic1Oyp8xiy78czlBG03jk/+/vdttJkie8pUc9AHBuMxAaV4iPN3zSi/J5OVSlovk607H3AUiL3Bfg4ssS1bsJvaFG0kuNscoiP+qLRTjFK6LzZS99VxegeNzttqGbtj5BwNgbtuzrIyfLmYB/9VgEw+QdaQHvxoAvD0f7aYsaJ1R6rrqxo+1Pun7j1/h7kOCGB0UcHDLDw7gaP/wIDAQABo0IwQDAPBgNVHRMBAf8EBTADAQH/MB0GA1UdDgQWBBQwIoo6QzzUL/TcNVpLGrLdd3DAIzAOBgNVHQ8BAf8EBAMCAoQwDQYJKoZIhvcNAQELBQADggEBALb8QycRmauyC/HRWRxTbl0w231HTAVYizQqhFQFl3beSQIhexGik+H+B4ve2rv94QRD3LlraUp+J26wLG89EnSCuCo/OxPAq+lxO6hNf6oKJ+Y2f48awIOxolO0f89qX3KMIkABXwKbYUcd+SBHX5ZP1V9cvJEyH0s3Fq9ObysPCH2j2Hjgz3WMIffSFMaO0DIfh3eNnv9hKQwavUO7fL/jqhBl4QxI2gMySi0Ni7PgAlBgxBx6YUp59q/lzMgAf19GOEOvI7l4dA0bc9pdsm7OhimskvOUSZYi5Pz3n/i/cTVKKhlj6NyINkMXlXGgyM9vEBpdcIpOWn/1H5QVy8Q=".to_string()]),
                x509_sha1_fingerprint: Some("NEMyMEFCMzUwMTE1QTNBOUFDMEQ1ODczRjk5NzBGQzY4QTk1Q0ZEOQ".to_string()),
                x509_sha256_fingerprint: None,
            },
            algorithm: AlgorithmParameters::RSA(RSAKeyParameters {
                key_type: Default::default(),
                n: "zkM1QHcP0v8bmwQ2fd3Pj6unCTx5k8LsW9cuLtUhAjjzRGpSEwGCKEgi1ej2-0Cxcs1t0wzhO-zSv1TJbsDI0x862PIFEs3xkGqPZU6rfQMzvCmncAcMjuW7r_Zewm0s58oRGyic1Oyp8xiy78czlBG03jk_-_vdttJkie8pUc9AHBuMxAaV4iPN3zSi_J5OVSlovk607H3AUiL3Bfg4ssS1bsJvaFG0kuNscoiP-qLRTjFK6LzZS99VxegeNzttqGbtj5BwNgbtuzrIyfLmYB_9VgEw-QdaQHvxoAvD0f7aYsaJ1R6rrqxo-1Pun7j1_h7kOCGB0UcHDLDw7gaP_w".to_string(),
                e: "AQAB".to_string(),
            }),
        };
        let jose_key = JoseKey::from_jwk(jwk, None).await;
        keys.push(Box::new(jose_key));
        let keyset = Keyset::new(keys);
        let header = Header {
            typ: Some("JWT".to_string()),
            alg: Algorithm::RS256,
            cty: None,
            jku: None,
            jwk: None,
            kid: Some(String::from(
                "NEMyMEFCMzUwMTE1QTNBOUFDMEQ1ODczRjk5NzBGQzY4QTk1Q0ZEOQ",
            )),
            x5u: None,
            x5c: None,
            x5t: None,
            x5t_s256: None,
        };
        let payload = JwtPayload {
            iss: Some("https://dev-ejtl988w.auth0.com/".to_string()),
            aud: Some(Audience::Single("did:web:pds.ripperoni.com".to_string())),
            sub: Some(Sub::new("gZSyspCY5dI4h1Z3qpwpdb9T4UPdGD5k@clients").unwrap()),
            exp: Some(1572492847),
            nbf: None,
            iat: Some(1572406447),
            jti: None,
            htm: None,
            htu: None,
            ath: None,
            acr: None,
            azp: Some("gZSyspCY5dI4h1Z3qpwpdb9T4UPdGD5k".to_string()),
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
        // let signed_jwt = keyset.create_jwt(header, payload).await.unwrap();
        // let expected = SignedJwt::new("").unwrap();
        // assert_eq!(signed_jwt, expected)
    }
}
