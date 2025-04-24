use crate::jwk::key::Key;
use crate::jwk::{
    algorithm_as_string, JwkError, JwtHeader, JwtPayload, SignedJwt, VerifyOptions, VerifyResult,
};
use biscuit::jwa::{Algorithm, SignatureAlgorithm};
use biscuit::jwk::{PublicKeyUse, JWK};
use biscuit::{Empty, JWT};
use rocket::form::validate::Contains;

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
                Algorithm::Signature(SignatureAlgorithm::ES256),
                Algorithm::Signature(SignatureAlgorithm::ES384),
                // https://datatracker.ietf.org/doc/html/rfc7518#section-3.5
                Algorithm::Signature(SignatureAlgorithm::PS256),
                Algorithm::Signature(SignatureAlgorithm::PS384),
                Algorithm::Signature(SignatureAlgorithm::PS512),
                Algorithm::Signature(SignatureAlgorithm::HS256),
                Algorithm::Signature(SignatureAlgorithm::HS384),
                Algorithm::Signature(SignatureAlgorithm::HS512),
            ],
        }
    }

    pub fn size(&self) -> usize {
        self.keys.len()
    }

    // pub fn sign_algorithms(&self) -> Vec<Algorithm> {
    //     let mut algorithms = HashSet::new();
    //     for key in &self.keys {
    //         if let Some(r#use) = key.r#use() {
    //             if r#use == PublicKeyUse::Signature {
    //                 for alg in key.algorithms() {
    //                     algorithms.insert(alg);
    //                 }
    //             }
    //         }
    //     }
    //     Vec::from_iter(algorithms)
    // }

    pub fn public_jwks(&self) -> Vec<JWK<Empty>> {
        let mut result = vec![];
        for key in &self.keys {
            if let Some(jwk) = key.public_jwk() {
                result.push(jwk);
            }
        }
        result
    }

    pub fn private_jwks(&self) -> Vec<JWK<Empty>> {
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
        header: JwtHeader,
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
            Err(error) => return Err(JwkError::JwtCreateError(error.to_string())),
        };
        header.alg = Some(algorithm_as_string(alg));
        header.kid = key.kid();
        key.create_jwt(header, jwt_payload).await
    }

    pub async fn verify_jwt(
        &self,
        signed_jwt: SignedJwt,
        options: Option<VerifyOptions>,
    ) -> Result<VerifyResult, JwkError> {
        let encoded_jwt = JWT::<JwtPayload, JwtHeader>::new_encoded(signed_jwt.val().as_str());
        let header = encoded_jwt.unverified_header().unwrap();
        let kid = header.registered.key_id;

        let mut errors = vec![];

        let kids = match kid {
            None => None,
            Some(kid) => Some(vec![kid]),
        };
        let key_search = KeySearch {
            r#use: None,
            kid: kids,
            alg: Some(vec![Algorithm::Signature(header.registered.algorithm)]),
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
            println!("{}", errors.get(0).unwrap());
            Err(JwkError::JwtVerifyError("ERR_JWT_INVALID".to_string()))
        } else {
            Err(JwkError::JwtVerifyError("ERR_JWT_INVALID".to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::jwk::{JwtHeader, JwtPayload, Key, Keyset, SignedJwt, VerifyResult};
    use crate::jwk_jose::jose_key::JoseKey;
    use biscuit::jwa::{Algorithm, SignatureAlgorithm};
    use biscuit::jwk::{
        AlgorithmParameters, CommonParameters, EllipticCurve, EllipticCurveKeyParameters,
        EllipticCurveKeyType, JWK,
    };
    use biscuit::Empty;

    #[tokio::test]
    async fn test_verify_jwt() {
        let mut keys: Vec<Box<dyn Key>> = vec![];
        let jwk = JWK {
            common: CommonParameters {
                algorithm: Some(Algorithm::Signature(SignatureAlgorithm::ES256)),
                ..Default::default()
            },
            algorithm: AlgorithmParameters::EllipticCurve(EllipticCurveKeyParameters {
                key_type: EllipticCurveKeyType::EC,
                curve: EllipticCurve::P256,
                x: base64_url::decode("A04hGmnNyRzyQ7U8Mf0vImpmPWhUv-PpHXggrEjJ6U0").unwrap(),
                y: base64_url::decode("GV_74zLzH5jBHu_vuOxeNXW5SBH6B3TEN9zPDT7GuSw").unwrap(),
                d: None,
            }),
            additional: Empty {},
        };
        let jose_key = JoseKey::from_jwk(jwk.clone(), None).await;
        keys.push(Box::new(jose_key));
        let keyset = Keyset::new(keys);
        let token = SignedJwt::new("eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiJBMDRoR21uTnlSenlRN1U4TWYwdkltcG1QV2hVdi1QcEhYZ2dyRWpKNlUwIiwieSI6IkdWXzc0ekx6SDVqQkh1X3Z1T3hlTlhXNVNCSDZCM1RFTjl6UERUN0d1U3cifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ1MzA3MzE4LCJqdGkiOiJoNm5yNDJxbWJlOjM0bjgzb2hvMnJ3NWQiLCJodG0iOiJHRVQiLCJodHUiOiJodHRwczovL3Bkcy5yaXBwZXJvbmkuY29tL3hycGMvYXBwLmJza3kuYWN0b3IuZ2V0UHJvZmlsZT9hY3Rvcj1kaWQlM0FwbGMlM0FldG01aW56eGh5Mnd0bzI2Z2dwcnpnZ3MiLCJub25jZSI6Ik4wcm94eWtqQmJ6RzVJYjVGTkhvaVMtNFJXQjBaR3pldlJmbDhsOWYtUDgiLCJhdGgiOiJaVXFDLWtuR01zUzdTenI1SldoMUJwZ01MVTNiZ3lJcFRjZmhIdnZuQ0xzIn0.dyO0MS-7hBj_ru10IkyZcfbW0OhrfEvsCUmXBQFe74tamfAFqb86OFeSGERVwNNp1kodHzcp11Ffs_AUgYeU0w").unwrap();
        let result = keyset.verify_jwt(token, None).await.unwrap();
        let expected = VerifyResult {
            payload: JwtPayload {
                iss: Some("https://cleanfollow-bsky.pages.dev/client-metadata.json".to_string()),
                iat: Some(1745307318),
                jti: Some("h6nr42qmbe:34n83oho2rw5d".to_string()),
                htm: Some("GET".to_string()),
                htu: Some("https://pds.ripperoni.com/xrpc/app.bsky.actor.getProfile?actor=did%3Aplc%3Aetm5inzxhy2wto26ggprzggs".to_string()),
                ath: Some("ZUqC-knGMsS7Szr5JWh1BpgMLU3bgyIpTcfhHvvnCLs".to_string()),
                nonce: Some("N0roxykjBbzG5Ib5FNHoiS-4RWB0ZGzevRfl8l9f-P8".to_string()),
                ..Default::default()
            },
            protected_header: JwtHeader {
                alg: Some("ES256".to_string()),
                jwk: Some(jwk),
                typ: Some("dpop+jwt".to_string()),
                ..Default::default()
            },
        };
        assert_eq!(result, expected)
    }
}
