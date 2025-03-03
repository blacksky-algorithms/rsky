use crate::oauth_types::OAuthTokenType;

pub enum OAuthError {
    InvalidGrantError(String),
    InvalidRequestError(String),
    InvalidClientMetadataError(String),
    InvalidRedirectUriError(String),
    InvalidParametersError(String),
    UnauthorizedClientError(String),
    InvalidTokenError(OAuthTokenType, String),
    InvalidDpopKeyBindingError,
    InvalidDpopProofError(String),
    RuntimeError(String),
    AccessDeniedError(String),
    InvalidClientAuthMethod(String),
    AccountSelectionRequiredError,
    LoginRequiredError,
    ConsentRequiredError,
}
