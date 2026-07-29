use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ResolveHandleOutput {
    pub did: String,
}

/// Resolves DID to DID document. Does not bi-directionally verify handle.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ResolveDidOutput {
    /// The complete DID document for the identity.
    #[serde(rename = "didDoc")]
    pub did_doc: JsonValue,
}

/// com.atproto.identity.defs#identityInfo
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct IdentityInfo {
    pub did: String,
    /// The validated handle of the account; or 'handle.invalid' if the handle
    /// did not bi-directionally match the DID document.
    pub handle: String,
    /// The complete DID document for the identity.
    #[serde(rename = "didDoc")]
    pub did_doc: JsonValue,
}

/// Request that the server re-resolve an identity (DID and handle).
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RefreshIdentityInput {
    pub identifier: String,
}

/// Updates the current account's handle. Verifies handle validity, and updates did:plc document if
/// necessary. Implemented by PDS, and requires auth.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UpdateHandleInput {
    /// The new handle.
    pub handle: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignPlcOperationRequest {
    pub token: String,
    pub rotation_keys: Option<Vec<String>>,
    pub also_known_as: Option<Vec<String>>,
    pub verification_methods: Option<BTreeMap<String, String>>,
    pub services: Option<JsonValue>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetRecommendedDidCredentialsResponse {
    pub also_known_as: Vec<String>,
    pub verification_methods: JsonValue,
    pub rotation_keys: Vec<String>,
    pub services: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitPlcOperationRequest {
    pub operation: JsonValue,
}
