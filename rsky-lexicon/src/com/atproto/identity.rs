use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetRecommendedDidCredentialsResponse {
    pub also_known_as: Vec<String>,
    pub verification_methods: JsonValue,
    pub rotation_keys: Vec<String>,
    pub services: JsonValue,
}
