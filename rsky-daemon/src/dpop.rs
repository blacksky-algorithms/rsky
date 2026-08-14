//! DPoP proofs for the space credential this syncer holds (RFC 9449,
//! proposal 0016 as amended by proposals#99).
//!
//! A space credential reads every repo in its space and is presented to each
//! of their hosts in turn, so hosts require proof that the presenter holds the
//! key the credential was bound to. The binding is established at issuance:
//! the proof sent with `getSpaceCredential` carries the public key, and the
//! authority writes its thumbprint into the credential's `cnf.jkt`.
//!
//! The key is generated per process and never persisted. A credential is
//! minted, used, and dropped by the same run, so a key that outlived it would
//! only be one more secret to keep.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use rsky_oauth::jwk::{EcCurve, Jwk};
use rsky_oauth::jwt::{sign, JwtClaims, JwtHeader};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{DaemonError, Result};

pub const DPOP_TYP: &str = "dpop+jwt";

/// The syncer's proof-of-possession key.
pub struct DpopSigner {
    key: Jwk,
    counter: AtomicU64,
}

impl DpopSigner {
    /// A fresh P-256 key. Rejection-samples because not every 32-byte string
    /// is a valid scalar.
    pub fn generate() -> Result<Self> {
        for _ in 0..8 {
            let bytes: [u8; 32] = rand::random();
            if let Ok(key) = Jwk::from_private_key_bytes(EcCurve::P256, &bytes) {
                return Ok(Self {
                    key,
                    counter: AtomicU64::new(0),
                });
            }
        }
        Err(DaemonError::Xrpc(
            "could not generate a DPoP key".to_string(),
        ))
    }

    /// RFC 7638 thumbprint — what a credential minted for this signer carries
    /// in `cnf.jkt`.
    pub fn thumbprint(&self) -> String {
        self.key.thumbprint()
    }

    /// A proof for one request. `access_token` is the credential being
    /// presented; issuance has none, because a delegation token is a grant
    /// rather than a bound token.
    pub fn proof(&self, method: &str, url: &str, access_token: Option<&str>) -> Result<String> {
        let mut header = JwtHeader::new("ES256");
        header.typ = Some(DPOP_TYP.to_string());
        header.jwk = Some(self.key.to_public());
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| DaemonError::Xrpc(e.to_string()))?
            .as_secs();
        let mut claims = JwtClaims {
            iat: Some(now),
            jti: Some(format!(
                "{}-{}",
                now,
                self.counter.fetch_add(1, Ordering::SeqCst)
            )),
            ..Default::default()
        };
        claims.extra.insert("htm".to_string(), method.into());
        claims.extra.insert("htu".to_string(), url.into());
        if let Some(token) = access_token {
            let ath = URL_SAFE_NO_PAD.encode(Sha256::digest(token.as_bytes()));
            claims.extra.insert("ath".to_string(), ath.into());
        }
        sign(&header, &claims, &self.key).map_err(|e| DaemonError::Xrpc(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsky_oauth::jwt::decode;

    #[test]
    fn a_proof_carries_the_public_key_and_binds_method_and_url() {
        let signer = DpopSigner::generate().unwrap();
        let jwt = signer
            .proof(
                "GET",
                "https://host.example/xrpc/com.atproto.space.listRepos",
                None,
            )
            .unwrap();
        let decoded = decode(&jwt).unwrap();
        assert_eq!(decoded.header.typ.as_deref(), Some(DPOP_TYP));
        let jwk = decoded.header.jwk.expect("proof carries its own key");
        assert!(
            !jwk.is_private(),
            "a proof must never carry the private key"
        );
        assert_eq!(jwk.thumbprint(), signer.thumbprint());
        assert_eq!(decoded.claims.extra.get("htm").unwrap(), "GET");
        assert_eq!(
            decoded.claims.extra.get("htu").unwrap(),
            "https://host.example/xrpc/com.atproto.space.listRepos"
        );
        assert!(decoded.claims.extra.get("ath").is_none());
    }

    #[test]
    fn presenting_a_credential_hashes_it_into_ath() {
        let signer = DpopSigner::generate().unwrap();
        let jwt = signer
            .proof("GET", "https://host.example/xrpc/x", Some("cred.jwt"))
            .unwrap();
        let decoded = decode(&jwt).unwrap();
        let expected = URL_SAFE_NO_PAD.encode(Sha256::digest(b"cred.jwt"));
        assert_eq!(decoded.claims.extra.get("ath").unwrap(), &expected);
    }

    #[test]
    fn each_proof_is_single_use() {
        let signer = DpopSigner::generate().unwrap();
        let first = decode(&signer.proof("GET", "https://h/x", None).unwrap()).unwrap();
        let second = decode(&signer.proof("GET", "https://h/x", None).unwrap()).unwrap();
        assert_ne!(first.claims.jti, second.claims.jti);
    }
}
