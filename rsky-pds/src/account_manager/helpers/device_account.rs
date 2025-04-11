use crate::account_manager::helpers::account::{select_account_qb, AvailabilityFlags};
use crate::db::DbConn;
use crate::schema::pds::actor::dsl as ActorSchema;
use crate::schema::pds::device::dsl as DeviceSchema;
use crate::schema::pds::device_account::dsl as DeviceAccountSchema;
use anyhow::Result;
use diesel::*;
use diesel::{delete, JoinOnDsl, QueryDsl, RunQueryDsl};
use rsky_common;
use rsky_oauth::oauth_provider::device::device_data::DeviceData;
use rsky_oauth::oauth_provider::device::device_id::DeviceId;
use rsky_oauth::oauth_provider::oidc::sub::Sub;

pub fn select_account_info_qb(device_id: DeviceId) {
    unimplemented!()
    // let mut builder = select_account_qb(Some(AvailabilityFlags {
    //     include_taken_down: None,
    //     include_deactivated: Some(true),
    // }));
    // builder = builder.inner_join(
    //     DeviceAccountSchema::device_account.on(DeviceAccountSchema::did.eq(ActorSchema::did)),
    // );
    // builder = builder
    //     .inner_join(DeviceSchema::device.on(DeviceSchema::id.eq(DeviceAccountSchema::deviceId)));
    // builder = builder.filter(DeviceSchema::id.eq(device_id));
    // builder = builder.select((
    //     DeviceAccountSchema::authenticatedAt,
    //     DeviceAccountSchema::remember,
    //     DeviceAccountSchema::authorizedClients,
    // ));
    // return builder
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

pub async fn get_account_info(device_id: DeviceId, sub: Sub, db: &DbConn) -> Result<()> {
    let x = select_account_info_qb(device_id);
    unimplemented!()
}
