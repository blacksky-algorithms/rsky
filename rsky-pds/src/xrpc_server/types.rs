use crate::auth_verifier::AuthError;
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HandlerPipeThrough {
    pub encoding: String,
    pub buffer: Vec<u8>,
    pub headers: Option<BTreeMap<String, String>>,
}

#[derive(Error, Debug)]
pub enum XRPCError {
    #[error("pipethrough network error")]
    UpstreamFailure,
    #[error("{0}")]
    FailedResponse(String),
}

#[derive(Error, Debug)]
pub enum InvalidRequestError {
    #[error("could not resolve proxy did service url")]
    CannotResolveServiceUrl,
    #[error("could not resolve proxy did")]
    CannotResolveProxyDid,
    #[error("Invalid service url: `{0}")]
    InvalidServiceUrl(String),
    #[error("Method not found")]
    MethodNotFound,
    #[error("no service id specified")]
    NoServiceId,
    #[error("No service configured for `{0}`")]
    NoServiceConfigured(String),
    #[error("AuthError: `{0}`")]
    AuthError(AuthError),
    #[error("XRPCError: {0}")]
    XRPCError(XRPCError),
}
