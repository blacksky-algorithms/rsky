use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::helpers::{account, password};
use crate::db::sqlite::Db;
use anyhow::Result;
use rsky_common::time::from_str_to_micros;
use rsky_common::RFC3339_VARIANT;
use rsky_oauth::request::RequestData;
use rsky_oauth::store::{AccountInfo, DeviceData, OAuthStore};
use rsky_oauth::token::{TokenData, TokenInfo};
use rsky_oauth::types::{AuthorizationRequestParameters, ClientAuth};
use rsky_oauth::OAuthError;
use rusqlite::{params, OptionalExtension, Row};

/// [`OAuthStore`] implementation over the PDS account database, mirroring
/// the upstream `oauth-store.ts` semantics.
#[derive(Clone, Debug)]
pub struct PdsOAuthStore {
    db: Db,
}

impl PdsOAuthStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

fn server_error(err: impl std::fmt::Display) -> OAuthError {
    OAuthError::ServerError(err.to_string())
}

fn secs_to_iso(secs: u64) -> String {
    let dt =
        chrono::DateTime::from_timestamp(secs as i64, 0).expect("unix seconds within chrono range");
    format!("{}", dt.format(RFC3339_VARIANT))
}

fn iso_to_secs(iso: &str) -> Result<u64, OAuthError> {
    Ok(from_str_to_micros(iso).map_err(server_error)? as u64 / 1_000_000)
}

fn to_json<T: serde::Serialize>(value: &T) -> Result<String, OAuthError> {
    serde_json::to_string(value).map_err(server_error)
}

fn from_json<T: serde::de::DeserializeOwned>(json: &str) -> Result<T, OAuthError> {
    serde_json::from_str(json).map_err(server_error)
}

fn actor_to_account_info(actor: account::ActorAccount) -> AccountInfo {
    AccountInfo {
        did: actor.did,
        handle: actor.handle,
        email: actor.email,
        deactivated: actor.deactivated_at.is_some(),
    }
}

struct RequestRow {
    did: Option<String>,
    device_id: Option<String>,
    client_id: String,
    client_auth: String,
    parameters: String,
    expires_at: String,
    code: Option<String>,
}

impl RequestRow {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            did: row.get("did")?,
            device_id: row.get("deviceId")?,
            client_id: row.get("clientId")?,
            client_auth: row.get("clientAuth")?,
            parameters: row.get("parameters")?,
            expires_at: row.get("expiresAt")?,
            code: row.get("code")?,
        })
    }

    fn into_request_data(self) -> Result<RequestData, OAuthError> {
        Ok(RequestData {
            client_id: self.client_id,
            client_auth: from_json::<ClientAuth>(&self.client_auth)?,
            parameters: from_json::<AuthorizationRequestParameters>(&self.parameters)?,
            expires_at: iso_to_secs(&self.expires_at)?,
            device_id: self.device_id,
            did: self.did,
            code: self.code,
        })
    }
}

struct TokenRow {
    token_id: String,
    did: String,
    created_at: String,
    updated_at: String,
    expires_at: String,
    client_id: String,
    client_auth: String,
    device_id: Option<String>,
    parameters: String,
    code: Option<String>,
    current_refresh_token: Option<String>,
}

impl TokenRow {
    const COLUMNS: &'static str = "\"tokenId\", did, \"createdAt\", \"updatedAt\", \
        \"expiresAt\", \"clientId\", \"clientAuth\", \"deviceId\", parameters, code, \
        \"currentRefreshToken\"";

    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            token_id: row.get("tokenId")?,
            did: row.get("did")?,
            created_at: row.get("createdAt")?,
            updated_at: row.get("updatedAt")?,
            expires_at: row.get("expiresAt")?,
            client_id: row.get("clientId")?,
            client_auth: row.get("clientAuth")?,
            device_id: row.get("deviceId")?,
            parameters: row.get("parameters")?,
            code: row.get("code")?,
            current_refresh_token: row.get("currentRefreshToken")?,
        })
    }

    fn into_token_info(self) -> Result<TokenInfo, OAuthError> {
        Ok(TokenInfo {
            token_id: self.token_id,
            data: TokenData {
                created_at: iso_to_secs(&self.created_at)?,
                updated_at: iso_to_secs(&self.updated_at)?,
                expires_at: iso_to_secs(&self.expires_at)?,
                client_id: self.client_id,
                client_auth: from_json::<ClientAuth>(&self.client_auth)?,
                device_id: self.device_id,
                did: self.did,
                parameters: from_json::<AuthorizationRequestParameters>(&self.parameters)?,
                code: self.code,
            },
            current_refresh_token: self.current_refresh_token,
        })
    }
}

async fn find_token_where(
    db: &Db,
    condition: &'static str,
    value: String,
) -> Result<Option<TokenInfo>, OAuthError> {
    let row: Option<TokenRow> = db
        .run(move |conn| {
            Ok(conn
                .query_row(
                    &format!("SELECT {} FROM token WHERE {condition}", TokenRow::COLUMNS),
                    params![value],
                    TokenRow::from_row,
                )
                .optional()?)
        })
        .await
        .map_err(server_error)?;
    row.map(TokenRow::into_token_info).transpose()
}

#[async_trait::async_trait]
impl OAuthStore for PdsOAuthStore {
    async fn create_request(&self, id: &str, data: &RequestData) -> Result<(), OAuthError> {
        let id = id.to_string();
        let data = data.clone();
        let client_auth = to_json(&data.client_auth)?;
        let parameters = to_json(&data.parameters)?;
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO authorization_request \
                     (id, did, \"deviceId\", \"clientId\", \"clientAuth\", parameters, \
                      \"expiresAt\", code) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        id,
                        data.did,
                        data.device_id,
                        data.client_id,
                        client_auth,
                        parameters,
                        secs_to_iso(data.expires_at),
                        data.code,
                    ],
                )?;
                Ok(())
            })
            .await
            .map_err(server_error)
    }

    async fn read_request(&self, id: &str) -> Result<Option<RequestData>, OAuthError> {
        let id = id.to_string();
        let row: Option<RequestRow> = self
            .db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT did, \"deviceId\", \"clientId\", \"clientAuth\", parameters, \
                         \"expiresAt\", code FROM authorization_request WHERE id = ?1",
                        params![id],
                        RequestRow::from_row,
                    )
                    .optional()?)
            })
            .await
            .map_err(server_error)?;
        row.map(RequestRow::into_request_data).transpose()
    }

    async fn update_request(&self, id: &str, data: &RequestData) -> Result<(), OAuthError> {
        let id = id.to_string();
        let data = data.clone();
        let client_auth = to_json(&data.client_auth)?;
        let parameters = to_json(&data.parameters)?;
        let updated = self
            .db
            .run(move |conn| {
                Ok(conn.execute(
                    "UPDATE authorization_request SET did = ?2, \"deviceId\" = ?3, \
                     \"clientAuth\" = ?4, parameters = ?5, \"expiresAt\" = ?6, code = ?7 \
                     WHERE id = ?1",
                    params![
                        id,
                        data.did,
                        data.device_id,
                        client_auth,
                        parameters,
                        secs_to_iso(data.expires_at),
                        data.code,
                    ],
                )?)
            })
            .await
            .map_err(server_error)?;
        if updated == 0 {
            return Err(OAuthError::ServerError("unknown request".to_string()));
        }
        Ok(())
    }

    async fn delete_request(&self, id: &str) -> Result<(), OAuthError> {
        let id = id.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM authorization_request WHERE id = ?1",
                    params![id],
                )?;
                Ok(())
            })
            .await
            .map_err(server_error)
    }

    async fn consume_request_code(
        &self,
        code: &str,
    ) -> Result<Option<(String, RequestData)>, OAuthError> {
        let code = code.to_string();
        let row: Option<(String, RequestRow)> = self
            .db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        "DELETE FROM authorization_request WHERE code = ?1 AND code IS NOT NULL \
                         RETURNING id, did, \"deviceId\", \"clientId\", \"clientAuth\", \
                         parameters, \"expiresAt\", code",
                        params![code],
                        |row| Ok((row.get("id")?, RequestRow::from_row(row)?)),
                    )
                    .optional()?)
            })
            .await
            .map_err(server_error)?;
        row.map(|(id, row)| Ok((id, row.into_request_data()?)))
            .transpose()
    }

    async fn create_token(
        &self,
        token_id: &str,
        data: &TokenData,
        refresh_token: Option<&str>,
    ) -> Result<(), OAuthError> {
        let token_id = token_id.to_string();
        let data = data.clone();
        let refresh_token = refresh_token.map(String::from);
        let client_auth = to_json(&data.client_auth)?;
        let parameters = to_json(&data.parameters)?;
        self.db
            .tx(move |tx| {
                if let Some(refresh_token) = &refresh_token {
                    let used: i64 = tx.query_row(
                        "SELECT COUNT(*) FROM used_refresh_token WHERE \"refreshToken\" = ?1",
                        params![refresh_token],
                        |row| row.get(0),
                    )?;
                    if used > 0 {
                        anyhow::bail!("refresh token already in use");
                    }
                }
                tx.execute(
                    "INSERT INTO token (did, \"tokenId\", \"createdAt\", \"updatedAt\", \
                     \"expiresAt\", \"clientId\", \"clientAuth\", \"deviceId\", parameters, \
                     details, code, \"currentRefreshToken\", scope) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, ?10, ?11, ?12)",
                    params![
                        data.did,
                        token_id,
                        secs_to_iso(data.created_at),
                        secs_to_iso(data.updated_at),
                        secs_to_iso(data.expires_at),
                        data.client_id,
                        client_auth,
                        data.device_id,
                        parameters,
                        data.code,
                        refresh_token,
                        data.parameters.scope,
                    ],
                )?;
                Ok(())
            })
            .await
            .map_err(server_error)
    }

    async fn read_token(&self, token_id: &str) -> Result<Option<TokenInfo>, OAuthError> {
        find_token_where(&self.db, "\"tokenId\" = ?1", token_id.to_string()).await
    }

    async fn find_token_by_refresh_token(
        &self,
        refresh_token: &str,
    ) -> Result<Option<TokenInfo>, OAuthError> {
        let refresh = refresh_token.to_string();
        let used_token_pk: Option<i64> = self
            .db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT \"tokenId\" FROM used_refresh_token WHERE \"refreshToken\" = ?1",
                        params![refresh],
                        |row| row.get(0),
                    )
                    .optional()?)
            })
            .await
            .map_err(server_error)?;
        match used_token_pk {
            Some(pk) => find_token_where(&self.db, "id = ?1", pk.to_string()).await,
            None => {
                find_token_where(
                    &self.db,
                    "\"currentRefreshToken\" = ?1",
                    refresh_token.to_string(),
                )
                .await
            }
        }
    }

    async fn find_token_by_code(&self, code: &str) -> Result<Option<TokenInfo>, OAuthError> {
        find_token_where(&self.db, "code = ?1 AND code IS NOT NULL", code.to_string()).await
    }

    async fn rotate_token(
        &self,
        token_id: &str,
        new_token_id: &str,
        new_refresh_token: &str,
        updated_at: u64,
        expires_at: u64,
    ) -> Result<(), OAuthError> {
        let token_id = token_id.to_string();
        let new_token_id = new_token_id.to_string();
        let new_refresh_token = new_refresh_token.to_string();
        self.db
            .tx(move |tx| {
                let row: Option<(i64, Option<String>)> = tx
                    .query_row(
                        "SELECT id, \"currentRefreshToken\" FROM token WHERE \"tokenId\" = ?1",
                        params![token_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()?;
                let Some((pk, current_refresh_token)) = row else {
                    anyhow::bail!("unknown token");
                };
                if let Some(current_refresh_token) = current_refresh_token {
                    tx.execute(
                        "INSERT INTO used_refresh_token (\"refreshToken\", \"tokenId\") \
                         VALUES (?1, ?2) ON CONFLICT DO NOTHING",
                        params![current_refresh_token, pk],
                    )?;
                }
                let reused: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM used_refresh_token WHERE \"refreshToken\" = ?1",
                    params![new_refresh_token],
                    |row| row.get(0),
                )?;
                if reused > 0 {
                    anyhow::bail!("new refresh token already in use");
                }
                tx.execute(
                    "UPDATE token SET \"tokenId\" = ?2, \"currentRefreshToken\" = ?3, \
                     \"updatedAt\" = ?4, \"expiresAt\" = ?5 WHERE id = ?1",
                    params![
                        pk,
                        new_token_id,
                        new_refresh_token,
                        secs_to_iso(updated_at),
                        secs_to_iso(expires_at),
                    ],
                )?;
                Ok(())
            })
            .await
            .map_err(server_error)
    }

    async fn delete_token(&self, token_id: &str) -> Result<(), OAuthError> {
        let token_id = token_id.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM token WHERE \"tokenId\" = ?1",
                    params![token_id],
                )?;
                Ok(())
            })
            .await
            .map_err(server_error)
    }

    async fn authenticate_account(
        &self,
        identifier: &str,
        password_str: &str,
    ) -> Result<Option<AccountInfo>, OAuthError> {
        let flags = Some(AvailabilityFlags {
            include_taken_down: None,
            include_deactivated: Some(true),
        });
        let identifier = identifier.to_ascii_lowercase();
        let found = if identifier.contains('@') {
            account::get_account_by_email(&identifier, flags, &self.db).await
        } else {
            account::get_account(&identifier, flags, &self.db).await
        }
        .map_err(server_error)?;
        let Some(actor) = found else {
            return Ok(None);
        };
        // OAuth sign-in only accepts the account password; app passwords
        // are rejected by never consulting them here.
        let valid =
            password::verify_account_password(&actor.did, &password_str.to_string(), &self.db)
                .await
                .map_err(server_error)?;
        Ok(valid.then(|| actor_to_account_info(actor)))
    }

    async fn get_account(&self, did: &str) -> Result<Option<AccountInfo>, OAuthError> {
        let found = account::get_account(
            did,
            Some(AvailabilityFlags {
                include_taken_down: None,
                include_deactivated: Some(true),
            }),
            &self.db,
        )
        .await
        .map_err(server_error)?;
        Ok(found.map(actor_to_account_info))
    }

    async fn create_device(&self, device_id: &str, data: &DeviceData) -> Result<(), OAuthError> {
        let device_id = device_id.to_string();
        let data = data.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO device (id, \"sessionId\", \"userAgent\", \"ipAddress\", \
                     \"lastSeenAt\") VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        device_id,
                        data.session_id,
                        data.user_agent,
                        data.ip_address,
                        secs_to_iso(data.last_seen_at),
                    ],
                )?;
                Ok(())
            })
            .await
            .map_err(server_error)
    }

    async fn read_device(&self, device_id: &str) -> Result<Option<DeviceData>, OAuthError> {
        let device_id = device_id.to_string();
        let row: Option<(String, Option<String>, String, String)> = self
            .db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT \"sessionId\", \"userAgent\", \"ipAddress\", \"lastSeenAt\" \
                         FROM device WHERE id = ?1",
                        params![device_id],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                    )
                    .optional()?)
            })
            .await
            .map_err(server_error)?;
        row.map(|(session_id, user_agent, ip_address, last_seen_at)| {
            Ok(DeviceData {
                session_id,
                user_agent,
                ip_address,
                last_seen_at: iso_to_secs(&last_seen_at)?,
            })
        })
        .transpose()
    }

    async fn update_device(&self, device_id: &str, data: &DeviceData) -> Result<(), OAuthError> {
        let device_id = device_id.to_string();
        let data = data.clone();
        let updated = self
            .db
            .run(move |conn| {
                Ok(conn.execute(
                    "UPDATE device SET \"sessionId\" = ?2, \"userAgent\" = ?3, \
                     \"ipAddress\" = ?4, \"lastSeenAt\" = ?5 WHERE id = ?1",
                    params![
                        device_id,
                        data.session_id,
                        data.user_agent,
                        data.ip_address,
                        secs_to_iso(data.last_seen_at),
                    ],
                )?)
            })
            .await
            .map_err(server_error)?;
        if updated == 0 {
            return Err(OAuthError::ServerError("unknown device".to_string()));
        }
        Ok(())
    }

    async fn upsert_device_account(&self, device_id: &str, did: &str) -> Result<(), OAuthError> {
        let device_id = device_id.to_string();
        let did = did.to_string();
        let now = rsky_common::now();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO account_device (did, \"deviceId\", \"createdAt\", \"updatedAt\") \
                     VALUES (?1, ?2, ?3, ?3) \
                     ON CONFLICT (\"deviceId\", did) DO UPDATE SET \"updatedAt\" = ?3",
                    params![did, device_id, now],
                )?;
                Ok(())
            })
            .await
            .map_err(server_error)
    }

    async fn get_device_account(
        &self,
        device_id: &str,
        did: &str,
    ) -> Result<Option<AccountInfo>, OAuthError> {
        let device_id = device_id.to_string();
        let did = did.to_string();
        let linked: Option<String> = self
            .db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT did FROM account_device WHERE \"deviceId\" = ?1 AND did = ?2",
                        params![device_id, did],
                        |row| row.get(0),
                    )
                    .optional()?)
            })
            .await
            .map_err(server_error)?;
        match linked {
            Some(did) => self.get_account(&did).await,
            None => Ok(None),
        }
    }

    async fn list_device_accounts(&self, device_id: &str) -> Result<Vec<AccountInfo>, OAuthError> {
        let device_id = device_id.to_string();
        let dids: Vec<String> = self
            .db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT did FROM account_device WHERE \"deviceId\" = ?1 \
                     ORDER BY \"updatedAt\" DESC",
                )?;
                let dids = stmt
                    .query_map(params![device_id], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<String>, rusqlite::Error>>()?;
                Ok(dids)
            })
            .await
            .map_err(server_error)?;
        let mut accounts = Vec::with_capacity(dids.len());
        for did in dids {
            if let Some(account) = self.get_account(&did).await? {
                accounts.push(account);
            }
        }
        Ok(accounts)
    }

    async fn remove_device_account(&self, device_id: &str, did: &str) -> Result<(), OAuthError> {
        let device_id = device_id.to_string();
        let did = did.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM account_device WHERE \"deviceId\" = ?1 AND did = ?2",
                    params![device_id, did],
                )?;
                Ok(())
            })
            .await
            .map_err(server_error)
    }

    async fn set_authorized_client(
        &self,
        did: &str,
        client_id: &str,
        scope: &str,
    ) -> Result<(), OAuthError> {
        let did = did.to_string();
        let client_id = client_id.to_string();
        let data = serde_json::json!({
            "authorizedScopes": scope.split_ascii_whitespace().collect::<Vec<&str>>(),
        })
        .to_string();
        let now = rsky_common::now();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO authorized_client (did, \"clientId\", \"createdAt\", \
                     \"updatedAt\", data) VALUES (?1, ?2, ?3, ?3, ?4) \
                     ON CONFLICT (did, \"clientId\") DO UPDATE SET \"updatedAt\" = ?3, data = ?4",
                    params![did, client_id, now, data],
                )?;
                Ok(())
            })
            .await
            .map_err(server_error)
    }

    async fn get_authorized_client_scope(
        &self,
        did: &str,
        client_id: &str,
    ) -> Result<Option<String>, OAuthError> {
        let did = did.to_string();
        let client_id = client_id.to_string();
        let data: Option<String> = self
            .db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT data FROM authorized_client WHERE did = ?1 AND \"clientId\" = ?2",
                        params![did, client_id],
                        |row| row.get(0),
                    )
                    .optional()?)
            })
            .await
            .map_err(server_error)?;
        data.map(|data| {
            let parsed: serde_json::Value = from_json(&data)?;
            let scopes = parsed["authorizedScopes"]
                .as_array()
                .map(|scopes| {
                    scopes
                        .iter()
                        .filter_map(|scope| scope.as_str())
                        .collect::<Vec<&str>>()
                        .join(" ")
                })
                .unwrap_or_default();
            Ok(scopes)
        })
        .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_manager::db::get_migrated_db;
    use crate::account_manager::tests::init_env;
    use crate::account_manager::{AccountManager, CreateAccountOpts};
    use lexicon_cid::Cid;
    use rsky_oauth::store::{DeviceData, OAuthStore};
    use std::str::FromStr;

    const NOW: u64 = 1_700_000_000;
    const DID: &str = "did:plc:oauthstoretest";
    const HANDLE: &str = "alice.test";

    async fn test_store() -> (tempfile::TempDir, PdsOAuthStore) {
        init_env();
        let dir = tempfile::tempdir().unwrap();
        let db = get_migrated_db(dir.path().join("account.sqlite"))
            .await
            .unwrap();
        let account_manager = AccountManager::new(db.clone());
        account_manager
            .create_account(CreateAccountOpts {
                did: DID.to_owned(),
                handle: HANDLE.to_owned(),
                email: Some("alice@example.com".to_owned()),
                password: Some("password123".to_owned()),
                repo_cid: Cid::from_str(
                    "bafkreibjfgx2gprinfvicegelk5kosd6y2frmqpqzwqkg7usac74l3t2v4",
                )
                .unwrap(),
                repo_rev: "3jzfcijpj2z2a".to_owned(),
                invite_code: None,
                deactivated: None,
            })
            .await
            .unwrap();
        (dir, PdsOAuthStore::new(db))
    }

    fn parameters() -> AuthorizationRequestParameters {
        AuthorizationRequestParameters {
            client_id: "https://app.example.com/client".to_string(),
            response_type: "code".to_string(),
            redirect_uri: "https://app.example.com/cb".to_string(),
            scope: "atproto transition:generic".to_string(),
            state: Some("state-1".to_string()),
            code_challenge: "challenge".to_string(),
            code_challenge_method: "S256".to_string(),
            login_hint: None,
            prompt: Some("consent".to_string()),
            dpop_jkt: Some("jkt-1".to_string()),
        }
    }

    fn request_data() -> RequestData {
        RequestData {
            client_id: "https://app.example.com/client".to_string(),
            client_auth: ClientAuth::None,
            parameters: parameters(),
            expires_at: NOW + 300,
            device_id: None,
            did: None,
            code: None,
        }
    }

    fn token_data() -> TokenData {
        TokenData {
            created_at: NOW,
            updated_at: NOW,
            expires_at: NOW + 3600,
            client_id: "https://app.example.com/client".to_string(),
            client_auth: ClientAuth::PrivateKeyJwt {
                alg: "ES256".to_string(),
                kid: "key-1".to_string(),
                jkt: "client-jkt".to_string(),
            },
            device_id: None,
            did: DID.to_string(),
            parameters: parameters(),
            code: Some("cod-1".to_string()),
        }
    }

    fn device_data() -> DeviceData {
        DeviceData {
            session_id: "ses-1".to_string(),
            user_agent: Some("test-agent".to_string()),
            ip_address: "127.0.0.1".to_string(),
            last_seen_at: NOW,
        }
    }

    #[tokio::test]
    async fn request_crud_roundtrip() {
        let (_dir, store) = test_store().await;
        assert!(store.read_request("req-1").await.unwrap().is_none());
        store
            .create_request("req-1", &request_data())
            .await
            .unwrap();
        assert_eq!(
            store.read_request("req-1").await.unwrap(),
            Some(request_data())
        );

        let mut updated = request_data();
        updated.did = Some(DID.to_string());
        updated.device_id = Some("dev-1".to_string());
        updated.code = Some("cod-abc".to_string());
        updated.expires_at = NOW + 600;
        store.update_request("req-1", &updated).await.unwrap();
        assert_eq!(
            store.read_request("req-1").await.unwrap(),
            Some(updated.clone())
        );
        assert!(store.update_request("req-missing", &updated).await.is_err());

        // consume by code is atomic delete + return
        let (id, data) = store
            .consume_request_code("cod-abc")
            .await
            .unwrap()
            .expect("code consumed");
        assert_eq!(id, "req-1");
        assert_eq!(data, updated);
        assert!(store.read_request("req-1").await.unwrap().is_none());
        assert!(store
            .consume_request_code("cod-abc")
            .await
            .unwrap()
            .is_none());

        store
            .create_request("req-2", &request_data())
            .await
            .unwrap();
        store.delete_request("req-2").await.unwrap();
        assert!(store.read_request("req-2").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn token_lifecycle_and_refresh_rotation() {
        let (_dir, store) = test_store().await;
        assert!(store.read_token("tok-1").await.unwrap().is_none());
        store
            .create_token("tok-1", &token_data(), Some("ref-1"))
            .await
            .unwrap();

        let info = store.read_token("tok-1").await.unwrap().unwrap();
        assert_eq!(info.token_id, "tok-1");
        assert_eq!(info.data, token_data());
        assert_eq!(info.current_refresh_token.as_deref(), Some("ref-1"));

        let by_code = store.find_token_by_code("cod-1").await.unwrap().unwrap();
        assert_eq!(by_code.token_id, "tok-1");
        let by_refresh = store
            .find_token_by_refresh_token("ref-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(by_refresh.token_id, "tok-1");

        store
            .rotate_token("tok-1", "tok-2", "ref-2", NOW + 100, NOW + 100 + 3600)
            .await
            .unwrap();
        assert!(store.read_token("tok-1").await.unwrap().is_none());
        let rotated = store.read_token("tok-2").await.unwrap().unwrap();
        assert_eq!(rotated.data.updated_at, NOW + 100);
        assert_eq!(rotated.data.created_at, NOW);
        assert_eq!(rotated.current_refresh_token.as_deref(), Some("ref-2"));

        // the rotated-out refresh token still resolves (replay detection)
        let replay = store
            .find_token_by_refresh_token("ref-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(replay.token_id, "tok-2");
        assert_eq!(replay.current_refresh_token.as_deref(), Some("ref-2"));

        // a used refresh token cannot back a new token
        assert!(store
            .create_token("tok-3", &token_data(), Some("ref-1"))
            .await
            .is_err());
        // rotating onto a used refresh token is rejected
        assert!(store
            .rotate_token("tok-2", "tok-4", "ref-1", NOW + 200, NOW + 200 + 3600)
            .await
            .is_err());
        assert!(store
            .rotate_token("tok-missing", "tok-5", "ref-5", NOW, NOW)
            .await
            .is_err());

        // deleting the token cascades used_refresh_token rows
        store.delete_token("tok-2").await.unwrap();
        assert!(store.read_token("tok-2").await.unwrap().is_none());
        assert!(store
            .find_token_by_refresh_token("ref-1")
            .await
            .unwrap()
            .is_none());
        assert!(store
            .find_token_by_refresh_token("ref-2")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn device_and_device_account_lifecycle() {
        let (_dir, store) = test_store().await;
        assert!(store.read_device("dev-1").await.unwrap().is_none());
        assert!(store.update_device("dev-1", &device_data()).await.is_err());
        store.create_device("dev-1", &device_data()).await.unwrap();
        assert_eq!(
            store.read_device("dev-1").await.unwrap(),
            Some(device_data())
        );
        let mut updated = device_data();
        updated.last_seen_at = NOW + 60;
        updated.user_agent = None;
        store.update_device("dev-1", &updated).await.unwrap();
        assert_eq!(store.read_device("dev-1").await.unwrap(), Some(updated));

        assert!(store
            .get_device_account("dev-1", DID)
            .await
            .unwrap()
            .is_none());
        store.upsert_device_account("dev-1", DID).await.unwrap();
        store.upsert_device_account("dev-1", DID).await.unwrap();
        let account = store
            .get_device_account("dev-1", DID)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(account.did, DID);
        assert_eq!(account.handle.as_deref(), Some(HANDLE));
        assert_eq!(store.list_device_accounts("dev-1").await.unwrap().len(), 1);
        assert!(store
            .list_device_accounts("dev-2")
            .await
            .unwrap()
            .is_empty());
        store.remove_device_account("dev-1", DID).await.unwrap();
        assert!(store
            .list_device_accounts("dev-1")
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn authenticate_and_get_account() {
        let (_dir, store) = test_store().await;
        let by_handle = store
            .authenticate_account(HANDLE, "password123")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(by_handle.did, DID);
        assert!(!by_handle.deactivated);
        let by_email = store
            .authenticate_account("ALICE@example.com", "password123")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(by_email.did, DID);
        assert!(store
            .authenticate_account(HANDLE, "wrong")
            .await
            .unwrap()
            .is_none());
        assert!(store
            .authenticate_account("nobody.test", "password123")
            .await
            .unwrap()
            .is_none());
        assert!(store
            .authenticate_account("nobody@example.com", "password123")
            .await
            .unwrap()
            .is_none());

        assert!(store.get_account(DID).await.unwrap().is_some());
        assert!(store
            .get_account("did:plc:missing")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn authorized_clients() {
        let (_dir, store) = test_store().await;
        assert!(store
            .get_authorized_client_scope(DID, "client-1")
            .await
            .unwrap()
            .is_none());
        store
            .set_authorized_client(DID, "client-1", "atproto")
            .await
            .unwrap();
        assert_eq!(
            store
                .get_authorized_client_scope(DID, "client-1")
                .await
                .unwrap()
                .as_deref(),
            Some("atproto")
        );
        store
            .set_authorized_client(DID, "client-1", "atproto transition:generic")
            .await
            .unwrap();
        assert_eq!(
            store
                .get_authorized_client_scope(DID, "client-1")
                .await
                .unwrap()
                .as_deref(),
            Some("atproto transition:generic")
        );
    }
}
