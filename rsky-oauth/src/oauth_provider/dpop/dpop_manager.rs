use crate::jwk::{decode_with_jwk, Jwk, JwtHeader, JwtPayload};
use crate::oauth_provider::dpop::dpop_nonce::{DpopNonce, DpopNonceError, DpopNonceInput};
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::now_as_secs;
use crate::oauth_provider::token::token_claims::TokenClaims;
use crate::oauth_provider::token::token_id::TokenId;
use crate::oauth_types::{Jwt, OAuthAccessToken};
use base64ct::{Base64, Encoding};
use biscuit::errors::Error;
use biscuit::jwk::JWKSet;
use biscuit::{ClaimsSet, Compact, ValidationOptions, JWT};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct DpopManagerOptions {
    /**
     * Set this to `false` to disable the use of nonces in DPoP proofs. Set this
     * to a secret Uint8Array or hex encoded string to use a predictable seed for
     * all nonces (typically useful when multiple instances are running). Leave
     * undefined to generate a random seed at startup.
     */
    pub dpop_secret: Option<DpopNonceInput>,
    pub dpop_step: Option<u64>,
}

#[derive(Clone)]
pub struct DpopManager {
    dpop_nonce: Option<Arc<RwLock<DpopNonce>>>,
}

impl DpopManager {
    pub fn new(opts: Option<DpopManagerOptions>) -> Result<Self, DpopNonceError> {
        match opts {
            None => Ok(DpopManager { dpop_nonce: None }),
            Some(opts) => {
                let dpop_nonce = DpopNonce::from(opts.dpop_secret, opts.dpop_step)?;
                Ok(DpopManager {
                    dpop_nonce: Some(Arc::new(RwLock::new(dpop_nonce))),
                })
            }
        }
    }

    pub async fn next_nonce(&mut self) -> Option<String> {
        match &self.dpop_nonce {
            None => None,
            Some(dpop_nonce) => {
                let mut dpop_nonce = dpop_nonce.write().await;
                Some(dpop_nonce.next())
            }
        }
    }

    /**
     * @see {@link https://datatracker.ietf.org/doc/html/rfc9449#section-4.3}
     */
    pub async fn check_proof(
        &self,
        proof: &str,
        htm: &str, // HTTP Method
        htu: &str, // HTTP URL
        access_token: Option<OAuthAccessToken>,
    ) -> Result<CheckProofResult, OAuthError> {
        let proof: biscuit::jws::Compact<ClaimsSet<TokenClaims>, JwtHeader> =
            JWT::new_encoded(proof);
        let header = match proof.unverified_header() {
            Ok(result) => result,
            Err(error) => return Err(OAuthError::InvalidDpopProofError(error.to_string())),
        };
        let embedded_jwk = match &header.registered.web_key {
            None => return Err(OAuthError::InvalidDpopProofError("Missing Jwk".to_string())),
            Some(jwk) => jwk.clone(),
        };

        let thumbprint = embedded_jwk
            .algorithm
            .thumbprint(&biscuit::digest::SHA256)
            .unwrap();

        let result: biscuit::jws::Compact<ClaimsSet<TokenClaims>, JwtHeader> =
            match decode_with_jwk(proof, embedded_jwk, None) {
                Ok(res) => res,
                Err(error) => {
                    panic!()
                }
            };

        let payload = result.payload().unwrap();
        let header = result.header().unwrap();

        if let Some(typ) = &header.registered.media_type {
            if typ != "dpop+jwt" {
                return Err(OAuthError::InvalidDpopProofError(
                    "Invalid \"typ\"".to_string(),
                ));
            }
        } else {
            return Err(OAuthError::InvalidDpopProofError(
                "Missing \"typ\"".to_string(),
            ));
        }

        if let Some(iat) = payload.registered.issued_at {
            if iat.timestamp() < now_as_secs() as i64 - 1000000 {
                return Err(OAuthError::InvalidDpopProofError(
                    "\"iat\" expired".to_string(),
                ));
            }
        } else {
            return Err(OAuthError::InvalidDpopProofError(
                "Missing \"iat\"".to_string(),
            ));
        }
        let payload_jti = match &payload.registered.id {
            None => {
                return Err(OAuthError::InvalidDpopProofError(
                    "Invalid or missing jti property".to_string(),
                ));
            }
            Some(jti) => jti.clone(),
        };

        // Note rfc9110#section-9.1 states that the method name is case-sensitive
        if let Some(payload_htm) = &payload.private.htm {
            if payload_htm != htm {
                return Err(OAuthError::InvalidDpopProofError(
                    "DPoP htm mismatch".to_string(),
                ));
            }
        }

        if payload.private.nonce.is_none() && self.dpop_nonce.is_some() {
            return Err(OAuthError::UseDpopNonceError(None));
        }

        if let Some(payload_nonce) = &payload.private.nonce {
            if let Some(dpop_nonce) = &self.dpop_nonce {
                let dpop_nonce = dpop_nonce.read().await;
                // if !dpop_nonce.check(payload_nonce) {
                //     return Err(OAuthError::UseDpopNonceError(Some(
                //         "DPoP nonce mismatch".to_string(),
                //     )));
                // }
            }
        }

        let htu_norm = match normalize_htu(htu.to_string()) {
            None => {
                return Err(OAuthError::InvalidRequestError(
                    "Invalid \"htu\" argument".to_string(),
                ));
            }
            Some(htu) => htu,
        };
        let payload_htu_norm =
            match normalize_htu(payload.private.htu.clone().unwrap_or("".to_string())) {
                None => {
                    return Err(OAuthError::InvalidRequestError(
                        "Invalid \"htu\" argument".to_string(),
                    ));
                }
                Some(htu) => htu,
            };
        if htu_norm != payload_htu_norm {
            return Err(OAuthError::InvalidDpopProofError(
                "DPoP htu mismatch".to_string(),
            ));
        }

        let payload_ath = payload.private.ath.clone();
        if let Some(access_token) = access_token {
            let hash = Sha256::digest(access_token.into_inner());
            let ath = Base64::encode_string(&hash);
            if let Some(payload_ath) = payload_ath {
                if payload_ath != ath {
                    return Err(OAuthError::InvalidDpopProofError(
                        "DPoP ath mismatch".to_string(),
                    ));
                }
            } else {
                return Err(OAuthError::InvalidDpopProofError(
                    "DPoP ath mismatch".to_string(),
                ));
            }
        } else if payload_ath.is_some() {
            return Err(OAuthError::InvalidDpopProofError(
                "DPoP ath not allowed".to_string(),
            ));
        }

        Ok(CheckProofResult {
            jti: payload_jti,
            jkt: thumbprint,
        })
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct CheckProofResult {
    pub jti: String,
    pub jkt: String,
}
/**
 * @note
 * > The htu claim matches the HTTP URI value for the HTTP request in which the
 * > JWT was received, ignoring any query and fragment parts.
 *
 * > To reduce the likelihood of false negatives, servers SHOULD employ
 * > syntax-based normalization (Section 6.2.2 of [RFC3986]) and scheme-based
 * > normalization (Section 6.2.3 of [RFC3986]) before comparing the htu claim.
 * > @see {@link https://datatracker.ietf.org/doc/html/rfc9449#section-4.3 | RFC9449 section 4.3. Checking DPoP Proofs}
 */
fn normalize_htu(htu: String) -> Option<String> {
    Some(htu)
    // if htu.is_empty() {
    //     return None
    // }
    //
    // let url = match Url::parse(htu.as_str()) {
    //     Ok(mut htu) => {
    //         htu.
    //     }
    //     Err(e) => {
    //         return None
    //     }
    // }
    // //TODO
    // Some(htu)
}

/**
 * Calculates a JSON Web Key (JWK) Thumbprint URI
 *
 * @param jwk JSON Web Key.
 * @param digestAlgorithm Digest Algorithm to use for calculating the thumbprint. Default is
 *   "sha256".
 *
 * @see {@link https://www.rfc-editor.org/rfc/rfc9278 RFC9278}
 */
fn calculate_jwk_thumbprint(jwk: Jwk) -> String {
    let data = serde_json::to_string(&jwk).unwrap();
    let hash = Sha256::digest(data);
    Base64::encode_string(&hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oauth_provider::dpop::dpop_manager::DpopManager;
    use crate::oauth_types::OAuthAccessToken;
    use rand::random;

    fn create_manager() -> DpopManager {
        let dpop_secret = random::<[u8; 32]>();
        let options = DpopManagerOptions {
            dpop_secret: Some(DpopNonceInput::Uint8Array(Vec::from(dpop_secret))),
            dpop_step: None,
        };
        DpopManager::new(Some(options)).unwrap()
    }

    #[tokio::test]
    async fn check_proof_no_nonce() {
        let manager = create_manager();
        let proof = "eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiJUbXR3WkNlUFQ0U1UtZDhEOUJjaDUxOUhfU3JweXFQTGhCNDl4UjhHLWY4IiwieSI6IkFMQjd2a04yNlhpeUtkOWNUTW01cElPMFdRMWZlNENqdXQwZGJETHBhbjgifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ0NjcxNDA3LCJqdGkiOiJoNmZtejlyMHE4OjJ1NjVtYnVyd3pxYjEiLCJodG0iOiJQT1NUIiwiaHR1IjoiaHR0cHM6Ly9wZHMucmlwcGVyb25pLmNvbS9vYXV0aC9wYXIifQ.CuECGFWJsDrGmNoA7uOXHFOENbzfzhk7ZW7NFePYG8Mc3lD5dgE-E8padaRDFT92chgWQKeZos9EWcZMt8CSUQ";
        let htm = "POST";
        let htu = "https://pds.ripperoni.com/oauth/par";
        let access_token: Option<OAuthAccessToken> = None;
        let result = manager
            .check_proof(proof, htm, htu, access_token)
            .await
            .unwrap_err();
        match result {
            OAuthError::UseDpopNonceError(error) => {}
            _ => {
                panic!()
            }
        }
    }

    #[tokio::test]
    async fn check_proof_with_nonce() {
        let manager = create_manager();
        let proof = "eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiJ6b3Q4YXIyTWtMcXJCeWh5a1laNzBHNnlXNXoybDhjM091dUZJRGJaOTBZIiwieSI6Imp6N0hLZmZKcTFUQ05uRE8zU3NpYXVkZG1acEY5TW1KWWlYR2M1YXZDSTgifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ1MDE1Njk1LCJqdGkiOiJoNmsxNTV3c2pjOmI4b2s3NmlxbzRwNiIsImh0bSI6IlBPU1QiLCJodHUiOiJodHRwczovL3Bkcy5yaXBwZXJvbmkuY29tL29hdXRoL3BhciIsIm5vbmNlIjoidTB5UW1MX1BoOVo3UkdUSkh1elVJZkFJNTlvOEtKYnFVNGY5UHM0R3BJWSJ9.PaquB0t9XWmZZXdjVY4uT-MU0IJez9guhGnE_wPx_WTAhHXEPHMzEt7eVyauuwajIuuT751Kg3Ul3fduZrxuAg";
        let htm = "POST";
        let htu = "https://pds.ripperoni.com/oauth/par";
        let access_token: Option<OAuthAccessToken> = None;
        let result = manager
            .check_proof(proof, htm, htu, access_token)
            .await
            .unwrap();
        let expected = CheckProofResult {
            jti: "h6k155wsjc:b8ok76iqo4p6".to_string(),
            jkt: "I9KDr9KrOZoXpUlfLHL1sDL4BV24uCYkXGIHqc7UD9E".to_string(),
        };
        assert_eq!(result, expected);
    }
}
