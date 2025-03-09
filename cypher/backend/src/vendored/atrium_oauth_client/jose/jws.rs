use jose_jwa::Algorithm;
use jose_jwk::Jwk;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    #[serde(flatten)]
    pub registered: RegisteredHeader,
}

impl From<Header> for super::Header {
    fn from(header: Header) -> Self {
        Self::Jws(header)
    }
}

// https://datatracker.ietf.org/doc/html/rfc7515#section-4.1
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisteredHeader {
    pub alg: Algorithm,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jku: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwk: Option<Jwk>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x5u: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x5c: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x5t: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "x5t#S256")]
    pub x5ts256: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub typ: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cty: Option<String>,
}

impl From<Algorithm> for RegisteredHeader {
    fn from(alg: Algorithm) -> Self {
        Self {
            alg,
            jku: None,
            jwk: None,
            kid: None,
            x5u: None,
            x5c: None,
            x5t: None,
            x5ts256: None,
            typ: None,
            cty: None,
        }
    }
}

impl From<RegisteredHeader> for super::Header {
    fn from(registered: RegisteredHeader) -> Self {
        Self::Jws(Header { registered })
    }
}
