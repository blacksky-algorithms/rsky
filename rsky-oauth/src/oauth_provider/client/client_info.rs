use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientInfo {
    /**
     * Defaults to `false`
     */
    pub is_first_party: bool,

    /**
     * Defaults to `true` if the client is isFirstParty, or if the client was
     * loaded from the store. (i.e. false in case of "loopback" & "discoverable"
     * clients)
     */
    pub is_trusted: bool,
}

impl Default for ClientInfo {
    fn default() -> Self {
        Self {
            is_first_party: false,
            is_trusted: true,
        }
    }
}
