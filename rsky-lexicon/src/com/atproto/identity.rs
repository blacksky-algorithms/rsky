use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ResolveHandleOutput {
    pub did: String,
}
