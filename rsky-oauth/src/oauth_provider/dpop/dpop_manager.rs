use crate::oauth_provider::dpop::dpop_nonce::{DpopNonce, DpopNonceError, DpopNonceInput};
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::now_as_secs;
use crate::oauth_provider::token::token_claims::TokenClaims;
use crate::oauth_provider::token::token_id::TokenId;
use crate::oauth_types::OAuthAccessToken;
use base64ct::{Base64, Encoding};
use jsonwebtoken::jwk::Jwk;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::str::FromStr;

pub struct DpopManagerOptions {
    /**
     * Set this to `false` to disable the use of nonces in DPoP proofs. Set this
     * to a secret Uint8Array or hex encoded string to use a predictable seed for
     * all nonces (typically useful when multiple instances are running). Leave
     * undefined to generate a random seed at startup.
     */
    dpop_secret: Option<DpopNonceInput>,
    dpop_step: Option<u64>,
}

#[derive(Clone)]
pub struct DpopManager {
    dpop_nonce: Option<DpopNonce>,
}

impl DpopManager {
    pub fn new(opts: Option<DpopManagerOptions>) -> Result<Self, DpopNonceError> {
        match opts {
            None => Ok(DpopManager { dpop_nonce: None }),
            Some(opts) => {
                let dpop_nonce = DpopNonce::from(opts.dpop_secret, opts.dpop_step)?;
                Ok(DpopManager {
                    dpop_nonce: Some(dpop_nonce),
                })
            }
        }
    }

    pub fn next_nonce(self) -> Option<String> {
        match self.dpop_nonce {
            None => None,
            Some(mut dpop_nonce) => Some(dpop_nonce.next()),
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
        validation.required_spec_claims = HashSet::new();
        let now = now_as_secs();
        let token_data = match decode::<TokenClaims>(proof.as_str(), &decoding_key, &validation) {
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
                    if iat < now - 10000 {
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
                if !dpop_nonce.check(payload_nonce) {
                    return Err(OAuthError::InvalidDpopProofError(
                        "DPoP nonce mismatch".to_string(),
                    )); //DPoP Nonce Error
                }
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
            payload,
            jti: payload_jti,
            jkt,
        })
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct CheckProofResult {
    pub payload: TokenClaims,
    pub jti: TokenId,
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
    use crate::oauth_provider::dpop::dpop_nonce::{DpopNonce, DpopNonceInput};
    use crate::oauth_provider::token::token_claims::TokenClaims;
    use crate::oauth_provider::token::token_id::TokenId;
    use crate::oauth_types::OAuthAccessToken;

    #[tokio::test]
    async fn check_proof() {
        let manager_options = DpopManagerOptions {
            dpop_secret: Some(DpopNonceInput::String(
                "1c9d92bea9a498e6165a39473e724a5d1c9d92bea9a498e6165a39473e724a5d".to_string(),
            )),
            dpop_step: Some(1),
        };
        let manager = DpopManager::new(Some(manager_options)).unwrap();
        let proof = "eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiJHY25oWVk5ekRKanJ6OVBRRjNTV0NLUzIxaDk3SWZRUEtUUm0wZzdUclMwIiwieSI6ImJKZEVFT1VWS2x3SFRsaXUzdUgzUkV1aDZfOF9VVkFEeWlEdlRJWmFNQnMifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ0MjQyNTU1LCJqdGkiOiJoNmE1eXU5MHd3OmFhMGU1a3ZsMTFtaCIsImh0bSI6IlBPU1QiLCJodHUiOiJodHRwczovL3Bkcy5yaXBwZXJvbmkuY29tL29hdXRoL3BhciIsIm5vbmNlIjoiaFRqNVduTEdqb1U4VGhTYVdJS0hTLUhaYm1pTmRkN0sxVkNpejJFN21pbyJ9.a7CCtgjcWAjue6qY5SgH5f_dwgKfYKIYTin9rejZgy5dcDGTt7RboxN_-z-HMK2JhIj6965wmXj8b7TF9z67bA";
        let htm = "POST";
        let htu = "https://pds.ripperoni.com/oauth/par";
        let access_token: Option<OAuthAccessToken> = None;
        let result = manager
            .check_proof(proof, htm, htu, access_token)
            .await
            .unwrap();
        let expected = CheckProofResult {
            payload: TokenClaims {
                iss: Some("https://cleanfollow-bsky.pages.dev/client-metadata.json".to_string()),
                aud: None,
                sub: None,
                exp: None,
                nbf: None,
                iat: Some(1744242555),
                jti: Some(TokenId::new("h6a5yu90ww:aa0e5kvl11mh".to_string()).unwrap()),
                htm: Some("POST".to_string()),
                htu: Some("https://pds.ripperoni.com/oauth/par".to_string()),
                ath: None,
                acr: None,
                azp: None,
                amr: None,
                cnf: None,
                client_id: None,
                scope: None,
                nonce: Some("hTj5WnLGjoU8ThSaWIKHS-HZbmiNdd7K1VCiz2E7mio".to_string()),
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
            },
            jti: TokenId::new("h6a5yu90ww:aa0e5kvl11mh").unwrap(),
            jkt: "YAGuktildGzIIuHWwUDE6R6U6EwF0oMgFXA3Hp2IH74=".to_string(),
        };
        assert_eq!(result, expected)
    }
}
