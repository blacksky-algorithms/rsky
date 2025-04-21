use crate::db::DbConn;
use crate::schema::pds::account::dsl as AccountSchema;
use crate::schema::pds::actor::dsl as ActorSchema;
use crate::schema::pds::device::dsl as DeviceSchema;
use crate::schema::pds::device_account::dsl as DeviceAccountSchema;
use anyhow::Result;
use chrono::{DateTime, Utc};
use diesel::*;
use diesel::{delete, QueryDsl, RunQueryDsl};
use rsky_common;
use rsky_common::now;
use rsky_oauth::jwk::Audience;
use rsky_oauth::oauth_provider::account::account::Account;
use rsky_oauth::oauth_provider::account::account_store::{AccountInfo, DeviceAccountInfo};
use rsky_oauth::oauth_provider::device::device_id::DeviceId;
use rsky_oauth::oauth_provider::oidc::sub::Sub;
use rsky_oauth::oauth_types::OAuthClientId;

pub async fn add_authorized_client(
    db: &DbConn,
    device_id: DeviceId,
    sub: Sub,
    client_id: OAuthClientId,
) -> Result<()> {
    //TODO
    // db.run(move |conn| {
    //     update(DeviceAccountSchema::device_account)
    //         .set(DeviceAccountSchema::authenticatedAt.eq())
    // }).await?:
    Ok(())
}

pub async fn remove_qb(device_id: DeviceId, sub: Sub, db: &DbConn) -> Result<()> {
    let device_id = device_id.into_inner();
    let did = sub.get();
    db.run(move |conn| {
        delete(DeviceAccountSchema::device_account)
            .filter(DeviceAccountSchema::deviceId.eq(device_id))
            .filter(DeviceAccountSchema::did.eq(did))
            .execute(conn)
    })
    .await?;

    Ok(())
}

pub async fn get_account_info(
    device_id: DeviceId,
    sub: Sub,
    audience: Audience,
    db: &DbConn,
) -> Result<Option<AccountInfo>> {
    let did = sub.get();
    let device_id = device_id.into_inner();
    let result = db
        .run(move |conn| {
            ActorSchema::actor
                .left_join(AccountSchema::account.on(ActorSchema::did.eq(AccountSchema::did)))
                .inner_join(
                    DeviceAccountSchema::device_account
                        .on(ActorSchema::did.eq(DeviceAccountSchema::did)),
                )
                .inner_join(
                    DeviceSchema::device.on(DeviceAccountSchema::deviceId.eq(DeviceSchema::id)),
                )
                .filter(ActorSchema::takedownRef.is_null())
                .filter(DeviceSchema::id.eq(device_id))
                .filter(ActorSchema::did.eq(did))
                .select((
                    ActorSchema::did,
                    ActorSchema::handle,
                    ActorSchema::createdAt,
                    ActorSchema::takedownRef,
                    ActorSchema::deactivatedAt,
                    ActorSchema::deleteAfter,
                    AccountSchema::email.nullable(),
                    AccountSchema::emailConfirmedAt.nullable(),
                    AccountSchema::invitesDisabled.nullable(),
                    DeviceAccountSchema::authenticatedAt,
                    DeviceAccountSchema::remember,
                    DeviceAccountSchema::authorizedClients,
                ))
                .first::<(
                    String,
                    Option<String>,
                    String,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<i16>,
                    DateTime<Utc>,
                    bool,
                    String,
                )>(conn)
                .optional()
        })
        .await?;
    let entry = match result {
        None => return Ok(None),
        Some(entry) => entry,
    };
    let sub = Sub::new(entry.0).unwrap();
    let aud = audience.clone();
    let email_verified = if entry.7.is_some() {
        Some(true)
    } else {
        Some(false)
    };
    let authorized_clients: Vec<OAuthClientId> =
        serde_json::from_str(entry.11.as_str()).unwrap_or(vec![]);
    let authenticated_at = entry.9;
    let account_info = AccountInfo {
        account: Account {
            sub,
            aud,
            preferred_username: entry.1,
            email: entry.6,
            email_verified,
            picture: None,
            name: None,
        },
        info: DeviceAccountInfo {
            remembered: entry.10,
            authenticated_at,
            authorized_clients,
        },
    };
    Ok(Some(account_info))
}

pub async fn read_qb(
    device_id: DeviceId,
    sub: Sub,
    db: &DbConn,
) -> Result<(bool, String, DateTime<Utc>)> {
    let did = sub.get();
    let device_id = device_id.into_inner();
    let result: (bool, String, DateTime<Utc>) = db
        .run(move |conn| {
            DeviceAccountSchema::device_account
                .filter(DeviceAccountSchema::did.eq(did))
                .filter(DeviceAccountSchema::deviceId.eq(device_id))
                .select((
                    DeviceAccountSchema::remember,
                    DeviceAccountSchema::authorizedClients,
                    DeviceAccountSchema::authenticatedAt,
                ))
                .first(conn)
        })
        .await?;
    Ok(result)
}

pub async fn list_remembered_devices(
    db: &DbConn,
    device_id: DeviceId,
    audience: Audience,
) -> Result<Vec<AccountInfo>> {
    let device_id = device_id.into_inner();
    let result = db
        .run(move |conn| {
            ActorSchema::actor
                .left_join(AccountSchema::account.on(ActorSchema::did.eq(AccountSchema::did)))
                .inner_join(
                    DeviceAccountSchema::device_account
                        .on(ActorSchema::did.eq(DeviceAccountSchema::did)),
                )
                .inner_join(
                    DeviceSchema::device.on(DeviceAccountSchema::deviceId.eq(DeviceSchema::id)),
                )
                .filter(ActorSchema::takedownRef.is_null())
                .filter(DeviceSchema::id.eq(device_id))
                .filter(DeviceAccountSchema::remember.eq(true))
                .select((
                    ActorSchema::did,
                    ActorSchema::handle,
                    ActorSchema::createdAt,
                    ActorSchema::takedownRef,
                    ActorSchema::deactivatedAt,
                    ActorSchema::deleteAfter,
                    AccountSchema::email.nullable(),
                    AccountSchema::emailConfirmedAt.nullable(),
                    AccountSchema::invitesDisabled.nullable(),
                    DeviceAccountSchema::authenticatedAt,
                    DeviceAccountSchema::remember,
                    DeviceAccountSchema::authorizedClients,
                ))
                // .load(conn)
                .load::<(
                    String,
                    Option<String>,
                    String,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<i16>,
                    DateTime<Utc>,
                    bool,
                    String,
                )>(conn)
        })
        .await?;
    let mut account_infos = vec![];
    for entry in result {
        let sub = Sub::new(entry.0).unwrap();
        let aud = audience.clone();
        let email_verified = if entry.7.is_some() {
            Some(true)
        } else {
            Some(false)
        };
        let authorized_clients: Vec<OAuthClientId> =
            serde_json::from_str(entry.11.as_str()).unwrap_or(vec![]);
        let account_info = AccountInfo {
            account: Account {
                sub,
                aud,
                preferred_username: entry.1,
                email: entry.6,
                email_verified,
                picture: None,
                name: None,
            },
            info: DeviceAccountInfo {
                remembered: entry.10,
                authenticated_at: entry.9,
                authorized_clients,
            },
        };
        account_infos.push(account_info.clone());
    }
    Ok(account_infos)
}

pub async fn create_or_update(
    db: &DbConn,
    device_id: DeviceId,
    sub: Sub,
    remember: bool,
) -> Result<()> {
    let device_id = device_id.into_inner();
    let did = sub.get();
    let authenticated_at = Utc::now();

    let authorized_clients: Vec<OAuthClientId> = vec![];
    let authorized_clients = serde_json::to_string(&authorized_clients)?;
    db.run(move |conn| {
        insert_into(DeviceAccountSchema::device_account)
            .values((
                DeviceAccountSchema::did.eq(&did),
                DeviceAccountSchema::deviceId.eq(&device_id),
                DeviceAccountSchema::authorizedClients.eq(&authorized_clients),
                DeviceAccountSchema::remember.eq(&remember),
                DeviceAccountSchema::authenticatedAt.eq(&authenticated_at),
            ))
            .on_conflict((DeviceAccountSchema::deviceId, DeviceAccountSchema::did))
            .do_update()
            .set((
                DeviceAccountSchema::deviceId.eq(&device_id),
                DeviceAccountSchema::did.eq(&did),
            ))
            .execute(conn)
    })
    .await?;
    Ok(())
}
