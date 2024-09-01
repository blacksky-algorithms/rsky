use serde_json::Value;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Could not resolve DID: `{0}`")]
    DidNotFoundError(String),
    #[error("Poorly formatted DID: `{0}`")]
    PoorlyFormattedDidError(String),
    #[error("Unsupported DID method: `{0}`")]
    UnsupportedDidMethodError(String),
    #[error("Poorly formatted DID Document: `{0:#?}`")]
    PoorlyFormattedDidDocumentError(Value),
    #[error("Unsupported did:web paths: `{0}`")]
    UnsupportedDidWebPathError(String),
}
