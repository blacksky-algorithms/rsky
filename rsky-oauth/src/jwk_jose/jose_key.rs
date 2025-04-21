use crate::jwk::{
    jwk_algorithms, JwkError, JwtPayload, Key, SignedJwt, VerifyOptions, VerifyResult,
};
use crate::oauth_provider::token::token_claims::TokenClaims;
use base64ct::{Base64, Encoding};
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
            let encoded_pass = Base64::encode_string("MHQCAQEEIABLnWmynwcZOjTh5Hpi/CbDhpf/ztbXYEwPFpa2jbj+oAcGBSuBBAAKoUQDQgAEzudE7Z+uxzpKnOuXGbf4axyj/3mV0+T7kuURWnheFv8b9e+3tpNX8ssGMPAt+1pHvAutIjPZem6SRP2mM6CfOw==".to_string().as_bytes());
            let encoding_key = EncodingKey::from_ec_pem(encoded_pass.as_bytes()).unwrap();
            // let encoding_key = EncodingKey::from_base64_secret(encoded_pass.as_str()).unwrap();
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
                        ..Default::default()
                    },
                    protected_header: result.header,
                }),
                Err(error) => Err(JwkError::JwtVerifyError(error.to_string())),
            };
        })
    }
}

impl JoseKey {
    //jsonwebtoken does not support encoding keys in their jwks, alternative will be needed down road
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
    use jsonwebtoken::jwk::{
        AlgorithmParameters, CommonParameters, EllipticCurveKeyParameters, EllipticCurveKeyType,
        OctetKeyParameters,
    };

    #[tokio::test]
    async fn test_create_jwt() {
        let jwk = Jwk {
            common: CommonParameters {
                public_key_use: Some(PublicKeyUse::Signature),
                key_algorithm: Some(KeyAlgorithm::ES256),
                key_id: Some("NEMyMEFCMzUwMTE1QTNBOUFDMEQ1ODczRjk5NzBGQzY4QTk1Q0ZEOQ".to_string()),
                ..Default::default()
            },
            algorithm: AlgorithmParameters::EllipticCurve(EllipticCurveKeyParameters {
                key_type: EllipticCurveKeyType::EC,
                curve: EllipticCurve::P256,
                x: "".to_string(),
                y: "".to_string(),
            }),
        };
        let jose_key = JoseKey::from_jwk(jwk.clone(), None).await;
        let header = Header {
            typ: Some("dpop+jwt".to_string()),
            alg: Algorithm::HS256,
            jwk: Some(jwk),
            ..Default::default()
        };

        let payload = JwtPayload {
            iss: Some("https://cleanfollow-bsky.pages.dev/client-metadata.json".to_string()),
            iat: Some(1745217238),
            jti: Some("h6mlqbjh4w:24v6qmav6v19u".to_string()),
            htm: Some("POST".to_string()),
            htu: Some("https://pds.ripperoni.com/oauth/par".to_string()),
            ..Default::default()
        };
        let signed_jwt = jose_key.create_jwt(header, payload).await.unwrap();
        let expected = SignedJwt::new("eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkhTMjU2In0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ1MjE3MjM4LCJqdGkiOiJoNm1scWJqaDR3OjI0djZxbWF2NnYxOXUiLCJodG0iOiJQT1NUIiwiaHR1IjoiaHR0cHM6Ly9wZHMucmlwcGVyb25pLmNvbS9vYXV0aC9wYXIifQ.KKlm0AvgitlDssgXKBqnd2F8nqBr7ZW7GBTxQq70FIs").unwrap();
        assert_eq!(signed_jwt, expected)
    }

    #[tokio::test]
    async fn test_verify_jwt() {
        let jwk = Jwk {
            common: CommonParameters {
                public_key_use: Some(PublicKeyUse::Signature),
                key_algorithm: Some(KeyAlgorithm::HS256),
                ..Default::default()
            },
            algorithm: AlgorithmParameters::OctetKey(OctetKeyParameters {
                key_type: Default::default(),
                value: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            }),
        };
        let jose_key = JoseKey::from_jwk(jwk, None).await;
        let token = SignedJwt::new("eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkhTMjU2In0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ1MjE3MjM4LCJqdGkiOiJoNm1scWJqaDR3OjI0djZxbWF2NnYxOXUiLCJodG0iOiJQT1NUIiwiaHR1IjoiaHR0cHM6Ly9wZHMucmlwcGVyb25pLmNvbS9vYXV0aC9wYXIifQ.KKlm0AvgitlDssgXKBqnd2F8nqBr7ZW7GBTxQq70FIs").unwrap();
        let verify_result = jose_key.verify_jwt(token, None).await.unwrap();
        let expected = VerifyResult {
            payload: JwtPayload {
                iss: Some("https://dev-ejtl988w.auth0.com/".to_string()),
                aud: Some(Audience::Single("did:web:pds.ripperoni.com".to_string())),
                sub: Some(Sub::new("gZSyspCY5dI4h1Z3qpwpdb9T4UPdGD5k@clients").unwrap()),
                exp: Some(1572492847),
                iat: Some(1572406447),
                azp: Some("gZSyspCY5dI4h1Z3qpwpdb9T4UPdGD5k".to_string()),
                ..Default::default()
            },
            protected_header: Header {
                typ: Some("JWT".to_string()),
                alg: Algorithm::RS256,
                kid: Some(String::from(
                    "NEMyMEFCMzUwMTE1QTNBOUFDMEQ1ODczRjk5NzBGQzY4QTk1Q0ZEOQ",
                )),
                ..Default::default()
            },
        };
        assert_eq!(verify_result, expected)
    }
}
