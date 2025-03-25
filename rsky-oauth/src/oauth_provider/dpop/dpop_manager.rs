use crate::oauth_provider::dpop::dpop_nonce::{DpopNonce, DpopNonceInput};
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::token::token_claims::TokenClaims;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};

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
        access_token: Option<String>,
    ) -> Result<(), OAuthError> {
        unimplemented!()
        // let decoding_key: DecodingKey;
        // let mut validation = Validation::new(Algorithm::HS256);
        // let token_data = decode::<TokenClaims>(proof.as_str(), &decoding_key, &validation).unwrap();
        // //TODO Change proof to unknown and validate
        //
        // let payload = token_data.claims;
        // let header = token_data.header;
        //
        // if payload.jti.is_none() {
        //     return Err(OAuthError::InvalidDpopProofError(
        //         "Invalid or missing jti property".to_string(),
        //     ));
        // }
        //
        // // Note rfc9110#section-9.1 states that the method name is case-sensitive
        // if let Some(payload_htm) = payload.htm {
        //     if payload_htm != htm {
        //         return Err(OAuthError::InvalidDpopProofError(
        //             "DPoP htm mismatch".to_string(),
        //         ));
        //     }
        // }
        //
        // if payload.nonce.is_none() && self.dpop_nonce.is_some() {
        //     return Err(OAuthError::InvalidDpopProofError(
        //         "DPoP htm mismatch".to_string(),
        //     )); //DPoP Nonce Error
        // }
        //
        // if let Some(payload_nonce) = payload.nonce {
        //     if !self.dpop_nonce.clone().unwrap().check(payload_nonce) {
        //         return Err(OAuthError::InvalidDpopProofError(
        //             "DPoP htm mismatch".to_string(),
        //         )); //DPoP Nonce Error
        //     }
        // }
        //
        // //htu norms
        //
        // if let Some(access_token) = access_token {
        //     // let ath_buffer = create_hash
        // } else if payload.ath.is_some() {
        //     return Err(OAuthError::InvalidDpopProofError(
        //         "DPoP ath not allowed".to_string(),
        //     )); //DPoP Nonce Error
        // }
        //
        // Ok(())
    }
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
