use serde::{Deserialize, Serialize};

/// Check accounts location in signup queue.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CheckSignupQueueOutput {
    pub activated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place_in_queue: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_time_ms: Option<i64>,
}
