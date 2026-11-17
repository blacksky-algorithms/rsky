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

/// `checkUserAccess`'s parameters as the 2026-10-29 spaces alpha names them
/// (bluesky-social/atproto `580a3b4a`): the user is `user`, not `did`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckUserAccessParams {
    pub space: String,
    pub user: String,
    pub access: Access,
    /// The attested `client_id`, when an attestation was presented; omitted
    /// for write checks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

/// `checkUserAccess`'s answer. The 2026-10-29 alpha names the decision
/// `authorized`; `allowed` is the name rsky read before, and the legacy
/// `simplespace.checkUserAccess` answers with, so it is still read. They are
/// two fields rather than one with a serde `alias`, so an answer carrying
/// both still parses; `authorized` wins. See [`Self::decision`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CheckUserAccessOutput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorized: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl CheckUserAccessOutput {
    /// The managing app's decision, or `None` when the answer carries
    /// neither name: not an answer at all.
    pub fn decision(&self) -> Option<bool> {
        self.authorized.or(self.allowed)
    }
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
                user: "did:plc:member".to_string(),
                access: Access::Write,
                client_id: Some("https://app.example.com/client-metadata.json".to_string()),
            },
            r#"{"space":"at://did:plc:auth/space/com.example.forum/self","user":"did:plc:member","access":"write","clientId":"https://app.example.com/client-metadata.json"}"#,
        );
        roundtrip(
            &CheckUserAccessOutput {
                authorized: Some(true),
                ..Default::default()
            },
            r#"{"authorized":true}"#,
        );
    }

    /// The alpha's `authorized`, the earlier `allowed`, both (where
    /// `authorized` wins), and neither, which is no decision.
    #[test]
    fn check_user_access_decision() {
        let read = |body: &str| {
            serde_json::from_str::<CheckUserAccessOutput>(body)
                .unwrap()
                .decision()
        };
        assert_eq!(read(r#"{"authorized":true}"#), Some(true));
        assert_eq!(
            read(r#"{"allowed":false,"reason":"not a follower"}"#),
            Some(false)
        );
        assert_eq!(read(r#"{"authorized":false,"allowed":true}"#), Some(false));
        assert_eq!(read(r#"{}"#), None);
    }
}
