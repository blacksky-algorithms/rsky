use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ResolveHandleOutput {
    pub did: String,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitPlcOperationRequest {
    pub operation: JsonValue,
}
