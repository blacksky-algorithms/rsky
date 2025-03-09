use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Claims {
    #[serde(flatten)]
    pub registered: RegisteredClaims,
    #[serde(flatten)]
    pub public: PublicClaims,
}

// https://datatracker.ietf.org/doc/html/rfc7519#section-4.1
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RegisteredClaims {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aud: Option<RegisteredClaimsAud>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nbf: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,
}

// https://www.iana.org/assignments/jwt/jwt.xhtml
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PublicClaims {
    // https://datatracker.ietf.org/doc/html/rfc9449#section-4.2
    #[serde(skip_serializing_if = "Option::is_none")]
    pub htm: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub htu: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ath: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
}

impl From<RegisteredClaims> for Claims {
    fn from(registered: RegisteredClaims) -> Self {
        Self {
            registered,
            public: PublicClaims::default(),
        }
    }
}

// https://datatracker.ietf.org/doc/html/rfc7519#section-4.1.3
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RegisteredClaimsAud {
    Single(String),
    #[allow(dead_code)]
    Multiple(Vec<String>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_claims() {
        // empty
        {
            let claims = Claims::default();
            let json = serde_json::to_string(&claims).expect("failed to serialize claims");
            assert_eq!(json, "{}");
        }
        // single aud
        {
            let claims = Claims {
                registered: RegisteredClaims {
                    aud: Some(RegisteredClaimsAud::Single(String::from("client"))),
                    ..Default::default()
                },
                public: PublicClaims::default(),
            };
            let json = serde_json::to_string(&claims).expect("failed to serialize claims");
            assert_eq!(json, r#"{"aud":"client"}"#);
        }
        // multiple auds
        {
            let claims = Claims {
                registered: RegisteredClaims {
                    aud: Some(RegisteredClaimsAud::Multiple(vec![
                        String::from("client1"),
                        String::from("client2"),
                    ])),
                    ..Default::default()
                },
                public: PublicClaims::default(),
            };
            let json = serde_json::to_string(&claims).expect("failed to serialize claims");
            assert_eq!(json, r#"{"aud":["client1","client2"]}"#);
        }
    }
}
