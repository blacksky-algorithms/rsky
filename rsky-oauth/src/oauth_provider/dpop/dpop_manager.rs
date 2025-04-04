use crate::oauth_provider::dpop::dpop_nonce::{DpopNonce, DpopNonceInput};
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::token::token_claims::TokenClaims;
use crate::oauth_types::OAuthAccessToken;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};

pub struct DpopManagerOptions {
    /**
     * Set this to `false` to disable the use of nonces in DPoP proofs. Set this
     * to a secret Uint8Array or hex encoded string to use a predictable seed for
     * all nonces (typically useful when multiple instances are running). Leave
     * undefined to generate a random seed at startup.
     */
    dpop_secret: Option<DpopNonceInput>,
    /**
     * Set this to `true` to disable the use of nonces in DPoP proofs
     */
    nonces_disabled: bool,
    dpop_step: Option<u64>,
}

#[derive(Clone)]
pub struct DpopManager {
    dpop_nonce: Option<DpopNonce>,
    nonces_disabled: bool,
}

impl DpopManager {
    pub fn new(opts: Option<DpopManagerOptions>) -> Self {
        match opts {
            None => DpopManager {
                dpop_nonce: None,
                nonces_disabled: false,
            },
            Some(opts) => DpopManager {
                dpop_nonce: Some(DpopNonce::from(opts.dpop_secret, opts.dpop_step)),
                nonces_disabled: false,
            },
        }
    }

    pub fn next_nonce(&self) -> Option<String> {
        self.dpop_nonce
            .clone()
            .map(|mut dpop_nonce| dpop_nonce.next())
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
        let header = decode_header(proof).unwrap();
        let jwk = header.jwk.unwrap();
        let decoding_key = DecodingKey::from_jwk(&jwk).unwrap();

        let validation = Validation::new(Algorithm::HS256);
        let token_data = decode::<TokenClaims>(proof.as_str(), &decoding_key, &validation).unwrap();

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
                "DPoP htm mismatch".to_string(),
            )); //DPoP Nonce Error
        }

        if let Some(payload_nonce) = &payload.nonce {
            if !self
                .dpop_nonce
                .clone()
                .unwrap()
                .check(payload_nonce.clone())
            {
                return Err(OAuthError::InvalidDpopProofError(
                    "DPoP htm mismatch".to_string(),
                )); //DPoP Nonce Error
            }
        }

        //htu norms

        if let Some(access_token) = access_token {
            // let ath_buffer = create_hash
        } else if payload.ath.is_some() {
            return Err(OAuthError::InvalidDpopProofError(
                "DPoP ath not allowed".to_string(),
            )); //DPoP Nonce Error
        }

        Ok(CheckProofResult {
            payload,
            jti: payload_jti,
            jkt: String::from(""),
        })
    }
}

pub struct CheckProofResult {
    pub payload: TokenClaims,
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
 * @see {@link https://datatracker.ietf.org/doc/html/rfc9449#section-4.3 | RFC9449 section 4.3. Checking DPoP Proofs}
 */
fn normalize_htu(htu: String) -> Option<String> {
    //TODO
    Some(htu)
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
fn calculate_jwk_thumbprint() {}
