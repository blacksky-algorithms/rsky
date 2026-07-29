use anyhow::{bail, Result};

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Account {
    pub did: String,
    pub email: String,
    #[serde(rename = "recoveryKey")]
    pub recovery_key: Option<String>,
    pub password: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "invitesDisabled")]
    pub invites_disabled: i16,
    #[serde(rename = "emailConfirmedAt")]
    pub email_confirmed_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Actor {
    pub did: String,
    pub handle: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "takedownRef")]
    pub takedown_ref: Option<String>,
    #[serde(rename = "deactivatedAt")]
    pub deactivated_at: Option<String>,
    #[serde(rename = "deleteAfter")]
    pub delete_after: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct AppPassword {
    pub did: String,
    pub name: String,
    pub password: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct DidDoc {
    pub did: String,
    pub doc: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum EmailTokenPurpose {
    #[default]
    ConfirmEmail,
    UpdateEmail,
    ResetPassword,
    DeleteAccount,
    PlcOperation,
}

impl EmailTokenPurpose {
    pub fn as_str(&self) -> &'static str {
        match self {
            EmailTokenPurpose::ConfirmEmail => "confirm_email",
            EmailTokenPurpose::UpdateEmail => "update_email",
            EmailTokenPurpose::ResetPassword => "reset_password",
            EmailTokenPurpose::DeleteAccount => "delete_account",
            EmailTokenPurpose::PlcOperation => "plc_operation",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "confirm_email" => Ok(EmailTokenPurpose::ConfirmEmail),
            "update_email" => Ok(EmailTokenPurpose::UpdateEmail),
            "reset_password" => Ok(EmailTokenPurpose::ResetPassword),
            "delete_account" => Ok(EmailTokenPurpose::DeleteAccount),
            "plc_operation" => Ok(EmailTokenPurpose::PlcOperation),
            _ => bail!("Unable to parse as EmailTokenPurpose: `{s:?}`"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct EmailToken {
    pub purpose: EmailTokenPurpose,
    pub did: String,
    pub token: String,
    #[serde(rename = "requestedAt")]
    pub requested_at: String,
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct InviteCode {
    pub code: String,
    #[serde(rename = "availableUses")]
    pub available_uses: i32,
    pub disabled: i16,
    #[serde(rename = "forAccount")]
    pub for_account: String,
    #[serde(rename = "createdBy")]
    pub created_by: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct InviteCodeUse {
    pub code: String,
    #[serde(rename = "usedBy")]
    pub used_by: String,
    #[serde(rename = "usedAt")]
    pub used_at: String,
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct RefreshToken {
    pub id: String,
    pub did: String,
    #[serde(rename = "expiresAt")]
    pub expires_at: String,
    #[serde(rename = "nextId")]
    pub next_id: Option<String>,
    #[serde(rename = "appPasswordName")]
    pub app_password_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct RepoSeq {
    pub seq: Option<i64>,
    pub did: String,
    #[serde(rename = "eventType")]
    pub event_type: String,
    pub event: Vec<u8>,
    pub invalidated: Option<i16>,
    #[serde(rename = "sequencedAt")]
    pub sequenced_at: String,
}

impl RepoSeq {
    pub fn new(did: String, event_type: String, event: Vec<u8>, sequenced_at: String) -> Self {
        RepoSeq {
            did,
            event_type,
            event,
            sequenced_at,
            invalidated: None, // default values used on insert
            seq: None,         // default values used on insert
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_token_purpose_round_trips() {
        let purposes = [
            EmailTokenPurpose::ConfirmEmail,
            EmailTokenPurpose::UpdateEmail,
            EmailTokenPurpose::ResetPassword,
            EmailTokenPurpose::DeleteAccount,
            EmailTokenPurpose::PlcOperation,
        ];
        for purpose in purposes {
            assert_eq!(
                EmailTokenPurpose::from_str(purpose.as_str()).unwrap(),
                purpose
            );
        }
        assert!(EmailTokenPurpose::from_str("bogus").is_err());
    }

    #[test]
    fn repo_seq_new_uses_insert_defaults() {
        let seq = RepoSeq::new(
            "did:plc:x".to_owned(),
            "append".to_owned(),
            vec![1, 2, 3],
            "2023-01-01T00:00:00.000Z".to_owned(),
        );
        assert_eq!(seq.seq, None);
        assert_eq!(seq.invalidated, None);
        assert_eq!(seq.event, vec![1, 2, 3]);
    }
}
