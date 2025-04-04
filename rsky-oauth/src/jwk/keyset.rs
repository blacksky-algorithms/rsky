use crate::jwk::{JwkError, JwtHeader, JwtPayload, SignedJwt, VerifyOptions, VerifyResult};
use jsonwebtoken::jwk::{Jwk, JwkSet};
use jsonwebtoken::Header;

#[derive(Clone)]
pub struct Keyset {
    keys: JwkSet,
}

pub struct KeySearch {
    pub r#use: Option<String>,
    pub kid: Option<Vec<String>>,
    pub alg: Option<Vec<String>>,
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

    async fn get(search: KeySearch) {}

    async fn list(search: KeySearch) {}

    async fn find_key(&self, header: JwtHeader, key_search: KeySearch) -> String {
        unimplemented!()
    }

    pub async fn create_jwt(&self, header: Header, jwt_payload: JwtPayload) -> SignedJwt {
        unimplemented!()
        // let key ;
        // jsonwebtoken::encode(&header, &jwt_payload, key).unwrap()
    }

    pub async fn verify_jwt(
        &self,
        signed_jwt: SignedJwt,
        options: Option<VerifyOptions>,
    ) -> Result<VerifyResult, JwkError> {
        unimplemented!()
        // let signed_jwt = signed_jwt.val();
        // let header = jsonwebtoken::decode_header(signed_jwt.as_str()).expect("No header");
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

        // let res = match jsonwebtoken::decode(token, &decoding_key, &validation) {
        //
        // };
    }
}
