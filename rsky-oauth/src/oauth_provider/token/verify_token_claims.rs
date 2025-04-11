use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::now_as_secs;
use crate::oauth_provider::token::token_claims::TokenClaims;
use crate::oauth_provider::token::token_id::TokenId;
use crate::oauth_types::{OAuthAccessToken, OAuthTokenType};

pub struct VerifyTokenClaimsOptions {
    pub audience: Option<Vec<String>>,
    pub scope: Option<Vec<String>>,
}

pub struct VerifyTokenClaimsResult {
    pub token: OAuthAccessToken,
    pub token_id: TokenId,
    pub token_type: OAuthTokenType,
    pub claims: TokenClaims,
}

pub fn verify_token_claims(
    token: OAuthAccessToken,
    token_id: TokenId,
    token_type: OAuthTokenType,
    dpop_jkt: Option<String>,
    claims: TokenClaims,
    options: Option<VerifyTokenClaimsOptions>,
) -> Result<VerifyTokenClaimsResult, OAuthError> {
    let date_reference = now_as_secs();
    let claims_jkt;
    match claims.cnf {
        None => {
            claims_jkt = None;
        }
        Some(ref cnf) => {
            claims_jkt = Some("test".to_string());
        }
    }

    let expected_token_type = match claims_jkt {
        None => OAuthTokenType::Bearer,
        Some(_) => OAuthTokenType::DPoP,
    };

    if expected_token_type != token_type {
        return Err(OAuthError::InvalidTokenError(
            token_type,
            "Invalid token type".to_string(),
        ));
    }
    if token_type == OAuthTokenType::DPoP && dpop_jkt.is_none() {
        return Err(OAuthError::InvalidDpopProofError(
            "jkt is required for DPoP tokens".to_string(),
        ));
    }
    if claims_jkt != dpop_jkt {
        return Err(OAuthError::InvalidDpopKeyBindingError);
    }

    if let Some(options) = options {
        if let Some(options_aud) = options.audience {
            if let Some(claims_aud) = claims.aud {
                return Err(OAuthError::InvalidTokenError(
                    token_type,
                    "Invalid audience".to_string(),
                ));
            }
        }

        if let Some(scope) = options.scope {
            return Err(OAuthError::InvalidTokenError(
                token_type,
                "Invalid scope".to_string(),
            ));
        }
    }

    if claims.exp.unwrap() * 1000 <= date_reference {
        return Err(OAuthError::InvalidTokenError(
            token_type,
            "Token expired".to_string(),
        ));
    }

    Ok(VerifyTokenClaimsResult {
        token,
        token_id,
        token_type,
        claims,
    })
}
