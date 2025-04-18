use crate::account_manager::helpers::account::{select_account_qb, AvailabilityFlags};
use crate::db::DbConn;
use crate::schema::pds::account::dsl as AccountSchema;
use crate::schema::pds::account::table as AccountTable;
use crate::schema::pds::actor::dsl as ActorSchema;
use crate::schema::pds::actor::table as ActorTable;
use crate::schema::pds::device::dsl as DeviceSchema;
use crate::schema::pds::device::dsl::device;
use crate::schema::pds::device::table as DeviceTable;
use crate::schema::pds::device_account::dsl as DeviceAccountSchema;
use crate::schema::pds::device_account::table as DeviceAccountTable;
use anyhow::Result;
use diesel::dsl::{exists, not, InnerJoinOn, LeftJoinOn};
use diesel::helper_types::{Eq, IntoBoxed};
use diesel::pg::Pg;
use diesel::*;
use diesel::{delete, QueryDsl, RunQueryDsl};
use rsky_common;
use rsky_oauth::oauth_provider::account::account_store::{AccountInfo, DeviceAccountInfo};
use rsky_oauth::oauth_provider::device::device_data::DeviceData;
use rsky_oauth::oauth_provider::device::device_id::DeviceId;
use rsky_oauth::oauth_provider::oidc::sub::Sub;
use rsky_oauth::oauth_types::OAuthClientId;

pub async fn add_authorized_client(
    db: &DbConn,
    device_id: DeviceId,
    sub: Sub,
    client_id: OAuthClientId,
) -> Result<()> {
    unimplemented!()
    // db.run(move |conn| {
    //     update(DeviceAccountSchema::device_account)
    //         .set(DeviceAccountSchema::authenticatedAt.eq())
    // }).await?:
}

pub fn select_account_info_qb(device_id: DeviceId) {
    unimplemented!()
    // let mut builder = select_account_qb(Some(AvailabilityFlags {
    //     include_taken_down: None,
    //     include_deactivated: Some(true),
    // }));
    // builder.into_boxed()
}

pub async fn list_device_accounts(device_id: DeviceId, db: &DbConn) -> Result<Option<DeviceData>> {
    unimplemented!()
    // let result = db
    //     .run(move |conn| {
    //         DeviceAccountSchema::device_account
    //             .filter(DeviceAccountSchema::code.eq(code))
    //             .select(models::AuthorizationRequest::as_select())
    //             .first(conn)
    //     })
    //     .await?;
    // Ok(Some(row_to_request(result)))
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
    db: &DbConn,
) -> Result<Option<AccountInfo>> {
    unimplemented!()
}

pub async fn read_qb(device_id: DeviceId, sub: Sub, db: &DbConn) -> Result<(bool, String, String)> {
    let did = sub.get();
    let device_id = device_id.into_inner();
    let result: (bool, String, String) = db
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

pub async fn list_remembered_devices(db: &DbConn, device_id: DeviceId) -> Result<Vec<AccountInfo>> {
    unimplemented!()
}

pub async fn create_or_update(
    db: &DbConn,
    device_id: DeviceId,
    sub: Sub,
    remember: bool,
) -> Result<()> {
    unimplemented!()
}
