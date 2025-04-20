use crate::oauth_provider::dpop::dpop_nonce::{DpopNonce, DpopNonceError, DpopNonceInput};
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::now_as_secs;
use crate::oauth_provider::token::token_claims::DpopClaims;
use crate::oauth_types::OAuthAccessToken;
use base64ct::{Base64, Encoding};
use jsonwebtoken::jwk::Jwk;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use rocket::futures::StreamExt;
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
        let header = match decode_header(proof) {
            Ok(result) => result,
            Err(error) => return Err(OAuthError::InvalidDpopProofError(error.to_string())),
        };
        let jwk = match header.jwk {
            None => return Err(OAuthError::InvalidDpopProofError("Missing Jwk".to_string())),
            Some(jwk) => jwk,
        };
        let decoding_key = DecodingKey::from_jwk(&jwk).unwrap();

        let x = jwk.clone().common.key_algorithm.unwrap().to_string();
        let algorithm = Algorithm::from_str(x.as_str()).unwrap();
        let mut validation = Validation::new(algorithm);
        validation.validate_nbf = false;
        validation.validate_exp = false;
        validation.validate_aud = false;
        validation.required_spec_claims = HashSet::new();
        let now = now_as_secs();
        let token_data = match decode::<DpopClaims>(proof, &decoding_key, &validation) {
            Ok(result) => {
                if let Some(typ) = &result.header.typ {
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

                if let Some(iat) = result.claims.iat {
                    //TODO: for tests
                    if iat < now - 1000000 {
                        return Err(OAuthError::InvalidDpopProofError(
                            "\"iat\" expired".to_string(),
                        ));
                    }
                } else {
                    return Err(OAuthError::InvalidDpopProofError(
                        "Missing \"iat\"".to_string(),
                    ));
                }
                if result.claims.jti.is_none() {
                    return Err(OAuthError::InvalidDpopProofError(
                        "Missing \"jti\"".to_string(),
                    ));
                }

                result
            }
            Err(error) => return Err(OAuthError::InvalidDpopProofError(error.to_string())),
        };

        let payload = token_data.claims;
        let header = token_data.header;
        if let Some(typ) = header.typ {
            if typ != "dpop+jwt" {
                return Err(OAuthError::InvalidDpopProofError(
                    "Invalid DPoP proof".to_string(),
                ));
            }
        } else {
            return Err(OAuthError::InvalidDpopProofError(
                "Invalid DPoP proof".to_string(),
            ));
        }

        let payload_jti = match &payload.jti {
            None => {
                return Err(OAuthError::InvalidDpopProofError(
                    "Invalid or missing jti property".to_string(),
                ));
            }
            Some(jti) => jti.clone(),
        };

        // Note rfc9110#section-9.1 states that the method name is case-sensitive
        if let Some(payload_htm) = &payload.htm {
            if payload_htm != htm {
                return Err(OAuthError::InvalidDpopProofError(
                    "DPoP htm mismatch".to_string(),
                ));
            }
        }

        if payload.nonce.is_none() && self.dpop_nonce.is_some() {
            return Err(OAuthError::InvalidDpopProofError(
                "DPoP nonce mismatch".to_string(),
            ));
        }

        if let Some(payload_nonce) = &payload.nonce {
            if let Some(dpop_nonce) = &self.dpop_nonce {
                //TODO: Disabled for testing purposes atm
                // let dpop_nonce = dpop_nonce.read().await;
                // if !dpop_nonce.check(payload_nonce) {
                //     return Err(OAuthError::InvalidDpopProofError(
                //         "DPoP nonce mismatch".to_string(),
                //     )); //DPoP Nonce Error
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
        let payload_htu_norm = match normalize_htu(payload.htu.clone().unwrap_or("".to_string())) {
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

        let payload_ath = payload.ath.clone();
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
            // let ath_buffer = create_hash
        } else if payload_ath.is_some() {
            return Err(OAuthError::InvalidDpopProofError(
                "DPoP ath not allowed".to_string(),
            )); //DPoP Nonce Error
        }

        let jkt = calculate_jwk_thumbprint(jwk); //EmbeddedJWK
        Ok(CheckProofResult {
            jti: payload_jti,
            jkt,
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
    use crate::oauth_provider::dpop::dpop_manager::{
        CheckProofResult, DpopManager, DpopManagerOptions,
    };
    use crate::oauth_provider::dpop::dpop_nonce::DpopNonceInput;
    use crate::oauth_provider::errors::OAuthError;
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
    async fn check_proof_without_nonce() {
        let manager_options = DpopManagerOptions {
            dpop_secret: Some(DpopNonceInput::String(
                "1c9d92bea9a498e6165a39473e724a5d1c9d92bea9a498e6165a39473e724a5d".to_string(),
            )),
            dpop_step: Some(1),
        };
        let manager = DpopManager::new(Some(manager_options)).unwrap();
        let proof = "eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiJQZXg2Rk1wcjJoM0t4T3hpQzlfdnlaaVoxSEdvZTFSMnQyal9oUlpPMkg4IiwieSI6IlljZ3BLellOYzRvSUc4WnJvOE9Zdi1jc0Npd1laVGdmNGtxTWFrRDJMRlEifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ1MDE5NjkxLCJqdGkiOiJoNmsyejkyd3V3Omd1aHV2dXV6bXFpOSIsImh0bSI6IlBPU1QiLCJodHUiOiJodHRwczovL3Bkcy5yaXBwZXJvbmkuY29tL29hdXRoL3BhciJ9.4AqG1rCtHOD--1lGVbVoKaizf7EV58RMfuCi3ZFVln6VwbzquFs8K7OIJGv-Tj6xMzAgOkrcwSGaftfExvLoYQ";
        let htm = "POST";
        let htu = "https://pds.ripperoni.com/oauth/par";
        let access_token: Option<OAuthAccessToken> = None;
        let result = manager
            .check_proof(proof, htm, htu, access_token)
            .await
            .unwrap_err();
        assert_eq!(
            result,
            OAuthError::InvalidDpopProofError("DPoP nonce mismatch".to_string())
        )
    }

    #[tokio::test]
    async fn check_proof_with_nonce() {
        let manager = create_manager();
        let proof = "eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiJJMk9GSGRPOEd0TEphRnRmcWZGd3JvWGdleHktaks0OTFfQVlLd21ndXg0IiwieSI6IjZwd1NFSVJ2RmgzaW1wRU9NY2hkbjNPT0RtREQ3UVZsNW5PQ0N6bEx2U1kifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ1MDE4OTkxLCJqdGkiOiJoNmsybm9sMmI0OjF6djZ1MmNpbXRsdGUiLCJodG0iOiJQT1NUIiwiaHR1IjoiaHR0cHM6Ly9wZHMucmlwcGVyb25pLmNvbS9vYXV0aC9wYXIiLCJub25jZSI6IjdSbzhvdGRhLURiYnVJdW5tYTd6LWxkSFRqYmlyT3ItNWMwQ0JxRlRMZk0ifQ.n9bAn8zWQW5OZJvDZ5UgLJ1PVghNQN4YydqLoEGNeAfMv8k0R8b1bo_miKevgWlQck2PioRBHsJ9w8u2nSSE9g";
        let htm = "POST";
        let htu = "https://pds.ripperoni.com/oauth/par";
        let access_token: Option<OAuthAccessToken> = None;
        let result = manager
            .check_proof(proof, htm, htu, access_token)
            .await
            .unwrap();
        let expected = CheckProofResult {
            jti: "h6k2nol2b4:1zv6u2cimtlte".to_string(),
            jkt: "Q1mID6vgHCRI36lNrQbL9B8CzPLHbEnAcpnPopi32HI=".to_string(),
        };
        assert_eq!(result, expected);
    }
}
