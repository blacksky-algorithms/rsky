use crate::vendored::atrium_oauth_client::jose::create_signed_jwt;
use crate::vendored::atrium_oauth_client::jose::jws::RegisteredHeader;
use crate::vendored::atrium_oauth_client::jose::jwt::Claims;
use jose_jwa::{Algorithm, Signing};
use jose_jwk::{Class, EcCurves, crypto};
use jose_jwk::{Jwk, JwkSet, Key};
use std::collections::HashSet;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("duplicate kid: {0}")]
    DuplicateKid(String),
    #[error("keys must not be empty")]
    EmptyKeys,
    #[error("key must have a `kid`")]
    EmptyKid,
    #[error("no signing key found for algorithms: {0:?}")]
    NotFound(Vec<String>),
    #[error("key for signing must be a secret key")]
    PublicKey,
    #[error("crypto error: {0:?}")]
    JwkCrypto(crypto::Error),
    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),
}

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Keyset(Vec<Jwk>);

impl Keyset {
    const PREFERRED_SIGNING_ALGORITHMS: [&'static str; 9] = [
        "EdDSA", "ES256K", "ES256", "PS256", "PS384", "PS512", "HS256", "HS384", "HS512",
    ];
    pub fn public_jwks(&self) -> JwkSet {
        let mut keys = Vec::with_capacity(self.0.len());
        for mut key in self.0.clone() {
            match key.key {
                Key::Ec(ref mut ec) => {
                    ec.d = None;
                }
                _ => unimplemented!(),
            }
            keys.push(key);
        }
        JwkSet { keys }
    }
    pub fn create_jwt(&self, algs: &[String], claims: Claims) -> Result<String> {
        let Some(jwk) = self.find_key(algs, Class::Signing) else {
            return Err(Error::NotFound(algs.to_vec()));
        };
        self.create_jwt_with_key(jwk, claims)
    }
    fn find_key(&self, algs: &[String], cls: Class) -> Option<&Jwk> {
        let candidates = self
            .0
            .iter()
            .filter_map(|key| {
                if key.prm.cls.map_or(false, |c| c != cls) {
                    return None;
                }
                let alg = match &key.key {
                    Key::Ec(ec) => match ec.crv {
                        EcCurves::P256 => "ES256",
                        _ => unimplemented!(),
                    },
                    _ => unimplemented!(),
                };
                Some((alg, key)).filter(|(alg, _)| algs.contains(&alg.to_string()))
            })
            .collect::<Vec<_>>();
        for pref_alg in Self::PREFERRED_SIGNING_ALGORITHMS {
            for (alg, key) in &candidates {
                if alg == &pref_alg {
                    return Some(key);
                }
            }
        }
        None
    }
    fn create_jwt_with_key(&self, key: &Jwk, claims: Claims) -> Result<String> {
        let kid = key.prm.kid.clone().unwrap();
        match crypto::Key::try_from(&key.key).map_err(Error::JwkCrypto)? {
            crypto::Key::P256(crypto::Kind::Secret(secret_key)) => {
                let mut header = RegisteredHeader::from(Algorithm::Signing(Signing::Es256));
                header.kid = Some(kid);
                Ok(create_signed_jwt(secret_key.into(), header.into(), claims)?)
            }
            _ => unimplemented!(),
        }
    }
}

impl TryFrom<Vec<Jwk>> for Keyset {
    type Error = Error;

    fn try_from(keys: Vec<Jwk>) -> Result<Self> {
        if keys.is_empty() {
            return Err(Error::EmptyKeys);
        }
        let mut v = Vec::with_capacity(keys.len());
        let mut hs = HashSet::with_capacity(keys.len());
        for key in keys {
            if let Some(kid) = key.prm.kid.clone() {
                if hs.contains(&kid) {
                    return Err(Error::DuplicateKid(kid));
                }
                hs.insert(kid);
                // ensure that the key is a secret key
                if match crypto::Key::try_from(&key.key).map_err(Error::JwkCrypto)? {
                    crypto::Key::P256(crypto::Kind::Public(_)) => true,
                    crypto::Key::P256(crypto::Kind::Secret(_)) => false,
                    _ => unimplemented!(),
                } {
                    return Err(Error::PublicKey);
                }
                v.push(key);
            } else {
                return Err(Error::EmptyKid);
            }
        }
        Ok(Self(v))
    }
}
