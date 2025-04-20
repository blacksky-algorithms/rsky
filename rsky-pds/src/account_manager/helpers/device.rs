use crate::db::DbConn;
use crate::models::models;
use anyhow::Result;
use diesel::{
    delete, insert_into, update, ExpressionMethods, QueryDsl, RunQueryDsl, SelectableHelper,
};
use rsky_common;
use rsky_oauth::oauth_provider::device::device_data::DeviceData;
use rsky_oauth::oauth_provider::device::device_id::DeviceId;
use rsky_oauth::oauth_provider::device::device_store::PartialDeviceData;
use rsky_oauth::oauth_provider::device::session_id::SessionId;

fn row_to_device_data(device: models::Device) -> DeviceData {
    DeviceData {
        user_agent: device.user_agent,
        ip_address: device.ip_address.parse().unwrap(),
        session_id: SessionId::new(device.session_id.unwrap()).unwrap(),
        last_seen_at: device.last_seen_at.parse().unwrap(),
    }
}

pub async fn create_device(device_id: DeviceId, data: DeviceData, db: &DbConn) -> Result<()> {
    use crate::schema::pds::device::dsl as DeviceSchema;
    db.run(move |conn| {
        let rows: Vec<models::Device> = vec![models::Device {
            id: device_id.into_inner(),
            session_id: Some(data.session_id.into_inner()),
            user_agent: data.user_agent,
            ip_address: data.ip_address.to_string(),
            last_seen_at: data.last_seen_at.to_string(),
        }];
        insert_into(DeviceSchema::device)
            .values(&rows)
            .execute(conn)
    })
    .await?;
    Ok(())
}

pub async fn read_device(device_id: DeviceId, db: &DbConn) -> Result<Option<DeviceData>> {
    use crate::schema::pds::device::dsl as DeviceSchema;

    let device_id = device_id.into_inner();
    let result = db
        .run(move |conn| {
            DeviceSchema::device
                .filter(DeviceSchema::id.eq(device_id))
                .select(models::Device::as_select())
                .first(conn)
        })
        .await?;
    Ok(Some(row_to_device_data(result)))
}

pub async fn update_device(
    device_id: DeviceId,
    opts: PartialDeviceData,
    db: &DbConn,
) -> Result<()> {
    use crate::schema::pds::device::dsl as DeviceSchema;
    db.run(move |conn| {
        //TODO
        // let mut update_list= vec![];
        // if let Some(user_agent) = opts.user_agent {
        //     update_list.push(DeviceSchema::user_agent.eq(user_agent));
        // };
        // if let Some(ip_address) = opts.ip_address {
        //     update_list.push(DeviceSchema::ip_address.eq(ip_address));
        // };
        // if let Some(session_id) = opts.session_id {
        //     update_list.push(DeviceSchema::session_id.eq(session_id));
        // }
        // if let Some(last_seen_at) = opts.last_seen_at {
        //     update_list.push(DeviceSchema::last_seen_at.eq(last_seen_at));
        // }
        // let update_tuples = update_list
        // update(DeviceSchema::device)
        //     .filter(DeviceSchema::id.eq(device_id))
        //     .set(update_list)
        //     .execute(conn)
    })
    .await;
    Ok(())
}

pub async fn delete_device(device_id: DeviceId, db: &DbConn) -> Result<()> {
    use crate::schema::pds::device::dsl as DeviceSchema;

    let device_id = device_id.into_inner();
    db.run(move |conn| {
        delete(DeviceSchema::device)
            .filter(DeviceSchema::id.eq(device_id))
            .execute(conn)
    })
    .await?;

    Ok(())
}

pub struct UpdateDeviceOpt {
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
    pub session_id: Option<SessionId>,
    pub last_seen_at: Option<u64>,
}
