use crate::db::DbConn;
use anyhow::Result;
use rsky_common;
use rsky_oauth::oauth_provider::device::device_data::DeviceData;
use rsky_oauth::oauth_provider::device::device_id::DeviceId;

pub fn create_device(device_id: DeviceId, data: DeviceData, db: &DbConn) -> Result<()> {
    unimplemented!()
}

pub fn read_device(device_id: DeviceId, db: &DbConn) -> Result<Option<DeviceData>> {
    unimplemented!()
}

pub fn update_device(device_id: DeviceId, db: &DbConn) -> Result<()> {
    unimplemented!()
}

pub fn remove_qb(db: &DbConn, device_id: DeviceId, sub: Sub) -> Result<()> {
    unimplemented!()
}
