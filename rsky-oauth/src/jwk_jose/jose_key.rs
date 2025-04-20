use crate::jwk::{
    jwk_algorithms, JwkError, JwtPayload, Key, SignedJwt, VerifyOptions, VerifyResult,
};
use crate::oauth_provider::token::token_claims::TokenClaims;
use jsonwebtoken::jwk::{AlgorithmParameters, EllipticCurve, Jwk, KeyAlgorithm, PublicKeyUse};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use std::future::Future;
use std::pin::Pin;

pub struct JoseKey {
    jwk: Jwk,
}

impl Key for JoseKey {
    fn is_private(&self) -> bool {
        match &self.jwk.algorithm {
            AlgorithmParameters::EllipticCurve(_) => false,
            AlgorithmParameters::RSA(_) => false,
            AlgorithmParameters::OctetKey(_) => true,
            AlgorithmParameters::OctetKeyPair(_) => false,
        }
    }

    fn is_symetric(&self) -> bool {
        match &self.jwk.algorithm {
            AlgorithmParameters::EllipticCurve(_) => false,
            AlgorithmParameters::RSA(_) => false,
            AlgorithmParameters::OctetKey(_) => true,
            AlgorithmParameters::OctetKeyPair(_) => false,
        }
    }

    fn private_jwk(&self) -> Option<Jwk> {
        if self.is_private() {
            Some(self.jwk.clone())
        } else {
            None
        }
    }

    fn public_jwk(&self) -> Option<Jwk> {
        if self.is_symetric() {
            return None;
        }

        // let mut jwk = self.jwk.clone();
        Some(self.jwk.clone())
    }

    fn bare_jwk(&self) -> Option<Jwk> {
        unimplemented!()
    }

    fn r#use(&self) -> Option<PublicKeyUse> {
        self.jwk.common.public_key_use.clone()
    }

    fn alg(&self) -> Option<KeyAlgorithm> {
        self.jwk.common.key_algorithm
    }

    fn kid(&self) -> Option<String> {
        self.jwk.common.key_id.clone()
    }

    fn crv(&self) -> Option<EllipticCurve> {
        match &self.jwk.algorithm {
            AlgorithmParameters::EllipticCurve(params) => Some(params.curve.clone()),
            AlgorithmParameters::RSA(_) => None,
            AlgorithmParameters::OctetKey(_) => None,
            AlgorithmParameters::OctetKeyPair(params) => Some(params.curve.clone()),
        }
    }

    fn algorithms(&self) -> Vec<Algorithm> {
        jwk_algorithms(&self.jwk)
    }

    fn create_jwt(
        &self,
        header: Header,
        payload: JwtPayload,
    ) -> Pin<Box<dyn Future<Output = Result<SignedJwt, JwkError>> + Send + Sync>> {
        let header = header.clone();
        let self_kid = self.jwk.common.key_id.clone();
        Box::pin(async move {
            if let Some(kid) = header.kid.clone() {
                if let Some(self_kid) = self_kid {
                    if kid != self_kid {
                        return Err(JwkError::JwtCreateError(format!(
                            "Invalid \"kid\" ({kid}) used to sign with key \"{self_kid}\""
                        )));
                    }
                }
            }
            let encoding_key = EncodingKey::from_base64_secret("").unwrap();
            match jsonwebtoken::encode(&header, &payload, &encoding_key) {
                Ok(result) => Ok(SignedJwt::new(result).unwrap()),
                Err(error) => Err(JwkError::JwtCreateError(error.to_string())),
            }
        })
    }

    fn verify_jwt(
        &self,
        token: SignedJwt,
        options: Option<VerifyOptions>,
    ) -> Pin<Box<dyn Future<Output = Result<VerifyResult, JwkError>> + Send + Sync>> {
        let token = token.clone().val();
        let options = Validation::new(Algorithm::HS256);
        let decoding_key = DecodingKey::from_jwk(&self.jwk).unwrap();

        Box::pin(async move {
            return match jsonwebtoken::decode::<TokenClaims>(&token, &decoding_key, &options) {
                Ok(result) => Ok(VerifyResult {
                    payload: JwtPayload {
                        iss: result.claims.iss,
                        aud: result.claims.aud,
                        sub: result.claims.sub,
                        exp: result.claims.exp,
                        nbf: result.claims.nbf,
                        iat: result.claims.iat,
                        jti: result.claims.jti,
                        htm: result.claims.htm,
                        htu: result.claims.htu,
                        ath: result.claims.ath,
                        acr: result.claims.acr,
                        azp: result.claims.azp,
                        amr: result.claims.amr,
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
                    },
                    protected_header: result.header,
                }),
                Err(error) => Err(JwkError::JwtVerifyError(error.to_string())),
            };
        })
    }
}

impl JoseKey {
    pub async fn from_jwk(jwk: Jwk, input_kid: Option<String>) -> Self {
        Self { jwk }
    }

    pub async fn from_key_like(key_like: String, kid: Option<String>, alg: Option<String>) -> Self {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jwk::Audience;
    use crate::oauth_provider::oidc::sub::Sub;
    use jsonwebtoken::jwk::{AlgorithmParameters, OctetKeyParameters};

    #[tokio::test]
    async fn test_create_jwt() {
        let jwk = Jwk {
            common: Default::default(),
            algorithm: AlgorithmParameters::OctetKey(OctetKeyParameters {
                key_type: Default::default(),
                value: "".to_string(),
            }),
        };
        let jose_key = JoseKey::from_jwk(jwk, None).await;
        let header = Header {
            typ: Some("dpop+jwt".to_string()),
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
        let signed_jwt = jose_key.create_jwt(header, payload).await.unwrap();
        let expected = SignedJwt::new("eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiJJMk9GSGRPOEd0TEphRnRmcWZGd3JvWGdleHktaks0OTFfQVlLd21ndXg0IiwieSI6IjZwd1NFSVJ2RmgzaW1wRU9NY2hkbjNPT0RtREQ3UVZsNW5PQ0N6bEx2U1kifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ1MDE4OTkxLCJqdGkiOiJoNmsybm9sMmI0OjF6djZ1MmNpbXRsdGUiLCJodG0iOiJQT1NUIiwiaHR1IjoiaHR0cHM6Ly9wZHMucmlwcGVyb25pLmNvbS9vYXV0aC9wYXIiLCJub25jZSI6IjdSbzhvdGRhLURiYnVJdW5tYTd6LWxkSFRqYmlyT3ItNWMwQ0JxRlRMZk0ifQ.n9bAn8zWQW5OZJvDZ5UgLJ1PVghNQN4YydqLoEGNeAfMv8k0R8b1bo_miKevgWlQck2PioRBHsJ9w8u2nSSE9g").unwrap();
        assert_eq!(signed_jwt, expected)
    }

    // #[tokio::test]
    // async fn test_verify_jwt() {
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
    //         algorithm: AlgorithmParameters::Octet,
    //     };
    //     let jose_key = JoseKey::from_jwk(jwk, None).await;
    //     let token = SignedJwt::new("eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiJJMk9GSGRPOEd0TEphRnRmcWZGd3JvWGdleHktaks0OTFfQVlLd21ndXg0IiwieSI6IjZwd1NFSVJ2RmgzaW1wRU9NY2hkbjNPT0RtREQ3UVZsNW5PQ0N6bEx2U1kifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ1MDE4OTkxLCJqdGkiOiJoNmsybm9sMmI0OjF6djZ1MmNpbXRsdGUiLCJodG0iOiJQT1NUIiwiaHR1IjoiaHR0cHM6Ly9wZHMucmlwcGVyb25pLmNvbS9vYXV0aC9wYXIiLCJub25jZSI6IjdSbzhvdGRhLURiYnVJdW5tYTd6LWxkSFRqYmlyT3ItNWMwQ0JxRlRMZk0ifQ.n9bAn8zWQW5OZJvDZ5UgLJ1PVghNQN4YydqLoEGNeAfMv8k0R8b1bo_miKevgWlQck2PioRBHsJ9w8u2nSSE9g").unwrap();
    //     let verify_options = VerifyOptions {
    //         audience: Some("http://helloworld".to_string()),
    //         clock_tolerance: None,
    //         issuer: None,
    //         max_token_age: None,
    //         subject: None,
    //         typ: None,
    //         current_date: None,
    //         required_claims: vec![],
    //     };
    //     let verify_result = jose_key.verify_jwt(token, Some(verify_options)).await.unwrap();
    //     let expected = VerifyResult {
    //         payload: JwtPayload {
    //             iss: Some("https://dev-ejtl988w.auth0.com/".to_string()),
    //             aud: Some(Audience::Single("did:web:pds.ripperoni.com".to_string())),
    //             sub: Some(Sub::new("gZSyspCY5dI4h1Z3qpwpdb9T4UPdGD5k@clients").unwrap()),
    //             exp: Some(1572492847),
    //             nbf: None,
    //             iat: Some(1572406447),
    //             jti: None,
    //             htm: None,
    //             htu: None,
    //             ath: None,
    //             acr: None,
    //             azp: Some("gZSyspCY5dI4h1Z3qpwpdb9T4UPdGD5k".to_string()),
    //             amr: None,
    //             cnf: None,
    //             client_id: None,
    //             scope: None,
    //             nonce: None,
    //             at_hash: None,
    //             c_hash: None,
    //             s_hash: None,
    //             auth_time: None,
    //             name: None,
    //             family_name: None,
    //             given_name: None,
    //             middle_name: None,
    //             nickname: None,
    //             preferred_username: None,
    //             gender: None,
    //             picture: None,
    //             profile: None,
    //             website: None,
    //             birthdate: None,
    //             zoneinfo: None,
    //             locale: None,
    //             updated_at: None,
    //             email: None,
    //             email_verified: None,
    //             phone_number: None,
    //             phone_number_verified: None,
    //             address: None,
    //             authorization_details: None,
    //             additional_claims: Default::default(),
    //         },
    //         protected_header: Header {
    //             typ: Some("JWT".to_string()),
    //             alg: Algorithm::RS256,
    //             cty: None,
    //             jku: None,
    //             jwk: None,
    //             kid: Some(String::from(
    //                 "NEMyMEFCMzUwMTE1QTNBOUFDMEQ1ODczRjk5NzBGQzY4QTk1Q0ZEOQ",
    //             )),
    //             x5u: None,
    //             x5c: None,
    //             x5t: None,
    //             x5t_s256: None,
    //         },
    //     };
    //     assert_eq!(verify_result, expected)
    // }
}
