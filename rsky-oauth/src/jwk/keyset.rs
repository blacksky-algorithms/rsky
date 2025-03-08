use crate::jwk::{JwkError, VerifyOptions, VerifyResult};
use jsonwebtoken::jwk::{Jwk, JwkSet};
use jsonwebtoken::{Algorithm, DecodingKey, Validation};

#[derive(Clone)]
pub struct Keyset {
    keys: JwkSet,
}

impl Keyset {
    pub fn new(keys: JwkSet) -> Self {
        Self { keys }
    }

    pub fn size(&self) -> usize {
        self.keys.keys.len()
    }

    pub fn public_jwks(&self) -> Vec<Jwk> {
        self.keys.keys.clone()
    }

    pub fn has(&self, kid: &str) -> bool {
        for key in &self.keys.keys {
            if let Some(j_kid) = &key.common.key_id {
                if j_kid == kid {
                    return true;
                }
            }
        }
        false
    }

    pub async fn verify_jwt(
        &self,
        token: &str,
        options: Option<VerifyOptions>,
    ) -> Result<VerifyResult, JwkError> {
        unimplemented!()
        // let header = jsonwebtoken::decode_header(token).expect("No header");
        // let kid = match header.kid {
        //     Some(kid) => kid,
        //     None => return Err(JwkError::JwtVerifyError),
        // };
        // let alg = header.alg;
        //
        // let key = match self.keys.find(kid.as_str()) {
        //     None => return Err(JwkError::JwtVerifyError),
        //     Some(jwk) => jwk.clone(),
        // };
        // let decoding_key = DecodingKey::from_jwk(&key).unwrap();
        //
        // let validation = Validation::new(alg);
        //
        // let res =
        //     jsonwebtoken::decode(token, &decoding_key, &validation).unwrap();
    }
}
