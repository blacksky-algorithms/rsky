use crate::db::DbConn;
use crate::schema::pds::account::dsl as AccountSchema;
use crate::schema::pds::actor::dsl as ActorSchema;
use crate::schema::pds::device_account::dsl as DeviceAccountSchema;
use crate::schema::pds::token::dsl as TokenSchema;
use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use diesel::*;
use diesel::{update, ExpressionMethods, NullableExpressionMethods, QueryDsl, RunQueryDsl};
use rsky_oauth::jwk::Audience;
use rsky_oauth::oauth_provider::account::account_store::DeviceAccountInfo;
use rsky_oauth::oauth_provider::client::client_auth::ClientAuth;
use rsky_oauth::oauth_provider::device::device_id::DeviceId;
use rsky_oauth::oauth_provider::oidc::sub::Sub;
use rsky_oauth::oauth_provider::request::code::Code;
use rsky_oauth::oauth_provider::token::refresh_token::RefreshToken;
use rsky_oauth::oauth_provider::token::token_data::TokenData;
use rsky_oauth::oauth_provider::token::token_id::TokenId;
use rsky_oauth::oauth_provider::token::token_store::{NewTokenData, TokenInfo};
use rsky_oauth::oauth_types::{
    OAuthAuthorizationDetails, OAuthAuthorizationRequestParameters, OAuthClientId,
};

pub async fn create_qb(
    db: &DbConn,
    token_id: TokenId,
    data: TokenData,
    refresh_token: Option<RefreshToken>,
) -> Result<()> {
    let token_id = token_id.val();
    let device_id = match data.device_id {
        None => None,
        Some(device_id) => Some(device_id.into_inner()),
    };
    let details = match data.details {
        None => None,
        Some(details) => Some(serde_json::to_string(&details)?),
    };
    let code = match data.code {
        None => None,
        Some(code) => Some(code.into_inner()),
    };
    let refresh_token = match refresh_token {
        None => None,
        Some(refresh_token) => Some(refresh_token.val()),
    };
    let did = data.sub.get();
    let parameters = serde_json::to_string(&data.parameters)?;
    let client_auth = serde_json::to_string(&data.client_auth)?;
    let client_id = data.client_id.into_inner();
    let updated_at = Utc::now();
    db.run(move |conn| {
        insert_into(TokenSchema::token)
            .values((
                TokenSchema::id.eq(&token_id),
                TokenSchema::tokenId.eq(&token_id),
                TokenSchema::createdAt.eq(data.created_at),
                TokenSchema::expiresAt.eq(data.expires_at),
                TokenSchema::updatedAt.eq(updated_at),
                TokenSchema::clientId.eq(client_id),
                TokenSchema::clientAuth.eq(client_auth),
                TokenSchema::deviceId.eq(device_id),
                TokenSchema::did.eq(did),
                TokenSchema::parameters.eq(parameters),
                TokenSchema::details.eq(details),
                TokenSchema::code.eq(code),
                TokenSchema::currentRefreshToken.eq(refresh_token),
            ))
            .execute(conn)
    })
    .await?;
    Ok(())
}

pub struct FindByQbOpts {
    pub id: Option<String>,
    pub code: Option<String>,
    pub token_id: Option<String>,
    pub current_refresh_token: Option<String>,
}

pub async fn read_token(
    db: &DbConn,
    opts: FindByQbOpts,
    audience: Audience,
) -> Result<Option<TokenInfo>> {
    if opts.current_refresh_token.is_none()
        && opts.token_id.is_none()
        && opts.code.is_none()
        && opts.id.is_none()
    {
        bail!("At least one search parameter is required");
    }

    let result = db
        .run(move |conn| {
            let mut builder = ActorSchema::actor
                .left_join(AccountSchema::account.on(ActorSchema::did.eq(AccountSchema::did)))
                .inner_join(TokenSchema::token.on(ActorSchema::did.eq(TokenSchema::did)))
                .left_join(DeviceAccountSchema::device_account.on(
                    DeviceAccountSchema::did.eq(TokenSchema::did).and(
                        DeviceAccountSchema::deviceId.eq(TokenSchema::deviceId.assume_not_null()),
                    ),
                ))
                .select((
                    //Actor
                    ActorSchema::did,
                    ActorSchema::handle.nullable(),
                    ActorSchema::createdAt,
                    ActorSchema::takedownRef.nullable(),
                    ActorSchema::deactivatedAt.nullable(),
                    ActorSchema::deleteAfter.nullable(),
                    //Account
                    AccountSchema::email.nullable(),
                    AccountSchema::emailConfirmedAt.nullable(),
                    AccountSchema::invitesDisabled.nullable(),
                    //Token
                    TokenSchema::tokenId,
                    TokenSchema::createdAt,
                    TokenSchema::updatedAt,
                    TokenSchema::expiresAt,
                    TokenSchema::clientId,
                    TokenSchema::clientAuth,
                    TokenSchema::deviceId.nullable(),
                    TokenSchema::did,
                    TokenSchema::parameters,
                    TokenSchema::details.nullable(),
                    TokenSchema::code.nullable(),
                    TokenSchema::currentRefreshToken.nullable(),
                    DeviceAccountSchema::authenticatedAt.nullable(),
                    DeviceAccountSchema::authorizedClients.nullable(),
                    DeviceAccountSchema::remember.nullable(),
                ))
                .into_boxed();

            if let Some(id) = opts.id {
                builder = builder.filter(TokenSchema::id.eq(id));
            }
            if let Some(code) = opts.code {
                builder = builder.filter(TokenSchema::code.eq(code));
            }
            if let Some(token_id) = opts.token_id {
                builder = builder.filter(TokenSchema::tokenId.eq(token_id));
            }
            if let Some(current_refresh_token) = opts.current_refresh_token {
                builder =
                    builder.filter(TokenSchema::currentRefreshToken.eq(current_refresh_token));
            }

            builder
                .first::<(
                    //Actor
                    String,
                    Option<String>,
                    String,
                    Option<String>,
                    Option<String>,
                    Option<String>,

                    //Account
                    Option<String>,
                    Option<String>,
                    Option<i16>,

                    //Token
                    String,
                    DateTime<Utc>,
                    DateTime<Utc>,
                    DateTime<Utc>,
                    String,
                    String,
                    Option<String>,
                    String,
                    String,
                    Option<String>,
                    Option<String>,
                    Option<String>,

                    //Device Account
                    Option<DateTime<Utc>>,
                    Option<String>,
                    Option<bool>,
                )>(conn)
                .optional()
        })
        .await?;

    let result = match result {
        None => return Ok(None),
        Some(result) => result,
    };

    let account_sub = Sub::new(result.0).unwrap();
    let handle = result.1;
    let email = result.7;
    let email_confirmed_at = result.8;
    let email_verified = match &email {
        None => None,
        Some(email) => Some(email_confirmed_at.is_some()),
    };
    let account = rsky_oauth::oauth_provider::account::account::Account {
        sub: account_sub,
        aud: audience,
        preferred_username: handle,
        email,
        email_verified,
        picture: None,
        name: None,
    };

    let token_id = TokenId::new(result.9)?;
    let token_created_at = result.10;
    let token_updated_at = result.11;
    let token_expires_at = result.12;
    let token_client_id = OAuthClientId::new(result.13)?;
    let token_client_auth: ClientAuth = serde_json::from_str(result.14.as_str())?;
    let token_did = match result.15 {
        None => None,
        Some(device_id) => Some(DeviceId::new(device_id)?),
    };
    let token_sub = Sub::new(result.16).unwrap();
    let token_parameters: OAuthAuthorizationRequestParameters =
        serde_json::from_str(result.17.as_str())?;
    let token_details: Option<OAuthAuthorizationDetails> = match result.18 {
        None => None,
        Some(details) => serde_json::from_str(details.as_str())?,
    };
    let token_code = match result.19 {
        None => None,
        Some(code) => Some(Code::new(code)?),
    };
    let current_refresh_token = match result.20 {
        None => None,
        Some(refresh_token) => Some(RefreshToken::new(refresh_token).unwrap()),
    };
    let device_account_info = if result.21.is_some() {
        let authenticated_at = result.21.unwrap();
        let x = result.22.unwrap();
        let authorized_clients: Vec<OAuthClientId> = serde_json::from_str(x.as_str())?;
        let remembered = result.23.unwrap();
        let info = DeviceAccountInfo {
            remembered,
            authenticated_at,
            authorized_clients,
        };
        Some(info)
    } else {
        None
    };
    let token_info = TokenInfo {
        id: token_id,
        data: TokenData {
            created_at: token_created_at,
            updated_at: token_updated_at,
            expires_at: token_expires_at,
            client_id: token_client_id,
            client_auth: token_client_auth,
            device_id: token_did,
            sub: token_sub,
            parameters: token_parameters,
            details: token_details,
            code: token_code,
        },
        account,
        info: device_account_info,
        current_refresh_token,
    };
    Ok(Some(token_info))
}

pub async fn remove_qb(db: &DbConn, token_id: TokenId) -> Result<()> {
    let token_id = token_id.into_inner();
    // uses "used_refresh_token_fk" to cascade delete
    db.run(move |conn| {
        delete(TokenSchema::token)
            .filter(TokenSchema::tokenId.eq(token_id))
            .execute(conn)
    })
    .await?;
    Ok(())
}

pub async fn for_rotate(db: &DbConn, token_id: String) -> Result<(String, String)> {
    let result = db
        .run(move |conn| {
            TokenSchema::token
                .filter(TokenSchema::tokenId.eq(token_id))
                .filter(TokenSchema::currentRefreshToken.is_not_null())
                .select((
                    TokenSchema::id,
                    TokenSchema::currentRefreshToken.assume_not_null(),
                ))
                .first::<(String, String)>(conn)
        })
        .await?;
    Ok(result)
}

pub async fn rotate_qb(
    db: &DbConn,
    id: String,
    new_token_id: TokenId,
    new_refresh_token: RefreshToken,
    new_data: NewTokenData,
) -> Result<()> {
    let new_token_id = new_token_id.into_inner();
    let new_refresh_token = new_refresh_token.val();
    let expires_at = new_data.expires_at;
    let updated_at = new_data.updated_at;
    let client_auth = serde_json::to_string(&new_data.client_auth)?;

    db.run(move |conn| {
        update(TokenSchema::token)
            .filter(TokenSchema::id.eq(id))
            .set((
                TokenSchema::tokenId.eq(new_token_id),
                TokenSchema::currentRefreshToken.eq(new_refresh_token),
                TokenSchema::expiresAt.eq(expires_at),
                TokenSchema::updatedAt.eq(updated_at),
                TokenSchema::clientAuth.eq(client_auth),
            ))
            .execute(conn)
    })
    .await?;
    Ok(())
}
