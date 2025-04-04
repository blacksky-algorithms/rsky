// // https://datatracker.ietf.org/doc/html/draft-ietf-oauth-v2-1-11#section-7.5.4
// const REDIRECT_STATUS_CODE: u64 = 303;
//
// pub struct AuthorizationResponseParameters {
//     // Will be added from AuthorizationResultRedirect['issuer']
//     // iss: string // rfc9207
//
//     // Will be added from AuthorizationResultRedirect['parameters']
//     // state?: string
//
//     pub code: Option<Code>,
//     pub id_token: Option<String>,
//     pub access_token: Option<String>,
//     pub token_type: Option<OAuthTokenType>,
//     pub expires_in: Option<String>,
//
//     pub response: Option<String>, // FAPI JARM
//     pub session_state: Option<String>, // OIDC Session Management
//
//     pub error: Option<String>,
//     pub error_description: Option<String>,
//     pub error_uri: Option<String>,
// }
//
// pub struct AuthorizationResultRedirect {
//     pub issuer: String,
//     pub parameters: OAuthAuthorizationRequestParameters,
//     pub reddirect: AuthorizationResponseParameters,
// }

use serde::{Deserialize, Serialize};

/// Authorization Result
///
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthorizationResult {
    Redirect,
    Authorize,
}
