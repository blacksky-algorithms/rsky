//! `com.atproto.managedspace.*` types: the baseline policy that defers each
//! user access decision to an external managing app.

/// `com.atproto.managedspace.policy`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Policy {
    pub managing_app: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publish_policy: Option<bool>,
}

/// The access kind a `checkUserAccess` call asks about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    Read,
    Write,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckUserAccessParams {
    pub space: String,
    pub did: String,
    pub access: Access,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckUserAccessOutput {
    pub allowed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::DeserializeOwned;
    use serde::Serialize;
    use std::fmt::Debug;

    const SPACE: &str = "at://did:plc:auth/space/com.example.forum/self";

    fn roundtrip<T>(value: &T, expected: &str)
    where
        T: Serialize + DeserializeOwned + PartialEq + Debug + Clone,
    {
        assert_eq!(serde_json::to_string(value).unwrap(), expected);
        assert_eq!(&serde_json::from_str::<T>(expected).unwrap(), value);
    }

    #[test]
    fn policy_pair() {
        roundtrip(
            &Policy {
                managing_app: "did:web:app.example.com#managing_app".to_string(),
                publish_policy: None,
            },
            r#"{"managingApp":"did:web:app.example.com#managing_app"}"#,
        );
        roundtrip(
            &Policy {
                managing_app: "did:web:app.example.com#managing_app".to_string(),
                publish_policy: Some(true),
            },
            r#"{"managingApp":"did:web:app.example.com#managing_app","publishPolicy":true}"#,
        );
    }

    #[test]
    fn check_user_access_pair() {
        roundtrip(
            &CheckUserAccessParams {
                space: SPACE.to_string(),
                did: "did:plc:member".to_string(),
                access: Access::Write,
                client_id: Some("https://app.example.com/client-metadata.json".to_string()),
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self","did":"did:plc:member","access":"write","clientId":"https://app.example.com/client-metadata.json"}"#,
        );
        roundtrip(
            &CheckUserAccessOutput {
                allowed: false,
                reason: Some("not a follower".to_string()),
            },
            r#"{"allowed":false,"reason":"not a follower"}"#,
        );
    }
}
