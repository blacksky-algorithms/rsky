use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use secp256k1::{Message, Secp256k1, SecretKey};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::{DaemonError, Result};

pub const MINT_LXM: &str = "community.blacksky.space.mintCredential";

pub struct ServiceJwtIssuer {
    did: String,
    secret: SecretKey,
}

impl ServiceJwtIssuer {
    pub fn from_hex(did: impl Into<String>, key: &str) -> Result<Self> {
        let bytes = hex::decode(key.trim()).map_err(|e| DaemonError::Xrpc(e.to_string()))?;
        let secret = SecretKey::from_slice(&bytes).map_err(|e| DaemonError::Xrpc(e.to_string()))?;
        Ok(Self {
            did: did.into(),
            secret,
        })
    }

    pub fn mint(&self, audience: &str, now: u64, jti: &str) -> Result<String> {
        #[derive(Serialize)]
        struct Header<'a> {
            typ: &'a str,
            alg: &'a str,
        }
        #[derive(Serialize)]
        struct Claims<'a> {
            iss: &'a str,
            aud: &'a str,
            exp: u64,
            lxm: &'a str,
            jti: &'a str,
            iat: u64,
        }
        let h = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&Header {
                typ: "JWT",
                alg: "ES256K",
            })
            .unwrap(),
        );
        let c = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&Claims {
                iss: &self.did,
                aud: audience,
                exp: now + 60,
                lxm: MINT_LXM,
                jti,
                iat: now,
            })
            .unwrap(),
        );
        let input = format!("{h}.{c}");
        let digest = Sha256::digest(input.as_bytes());
        let message =
            Message::from_digest_slice(&digest).map_err(|e| DaemonError::Xrpc(e.to_string()))?;
        let mut sig = Secp256k1::new().sign_ecdsa(&message, &self.secret);
        sig.normalize_s();
        Ok(format!(
            "{input}.{}",
            URL_SAFE_NO_PAD.encode(sig.serialize_compact())
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mints_a_short_lived_method_bound_token() {
        let issuer = ServiceJwtIssuer::from_hex("did:plc:daemon", &hex::encode([7u8; 32])).unwrap();
        let jwt = issuer.mint("did:plc:authority", 1000, "jti").unwrap();
        let claims: serde_json::Value = serde_json::from_slice(
            &URL_SAFE_NO_PAD
                .decode(jwt.split('.').nth(1).unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(claims["iss"], "did:plc:daemon");
        assert_eq!(claims["aud"], "did:plc:authority");
        assert_eq!(claims["lxm"], MINT_LXM);
        assert_eq!(claims["exp"], 1060);
    }
}
