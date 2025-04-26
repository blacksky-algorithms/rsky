use crate::oauth_types::CLIENT_ASSERTION_TYPE_JWT_BEARER;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone, Eq, PartialEq)]
pub enum ClientAuth {
    None,
    Some(ClientAuthDetails),
}

#[derive(Debug, Deserialize, Serialize, Clone, Eq, PartialEq)]
pub struct ClientAuthDetails {
    pub alg: String,
    pub kid: String,
    pub jkt: String,
}

impl ClientAuth {
    pub fn new(options: Option<ClientAuthDetails>) -> Self {
        match options {
            None => ClientAuth::None,
            Some(param_details) => ClientAuth::Some(ClientAuthDetails {
                alg: param_details.alg,
                kid: param_details.kid,
                jkt: param_details.jkt,
            }),
        }
    }

    pub fn is_none(&self) -> bool {
        match self {
            ClientAuth::None => true,
            ClientAuth::Some(_) => false,
        }
    }

    pub fn is_jwt_bearer(&self) -> bool {
        match self {
            ClientAuth::None => false,
            ClientAuth::Some(_) => true,
        }
    }

    pub fn method(&self) -> String {
        match self {
            ClientAuth::None => "none".to_string(),
            ClientAuth::Some(_) => CLIENT_ASSERTION_TYPE_JWT_BEARER.to_string(),
        }
    }
}
