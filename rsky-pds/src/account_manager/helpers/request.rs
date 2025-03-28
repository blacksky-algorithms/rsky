use crate::db::DbConn;
use anyhow::Result;
use rsky_common;
use rsky_oauth::oauth_provider::device::device_data::DeviceData;
use rsky_oauth::oauth_provider::request::code::Code;
use rsky_oauth::oauth_provider::request::request_data::RequestData;
use rsky_oauth::oauth_provider::request::request_id::RequestId;

pub fn create_request(id: RequestId, data: RequestData, db: &DbConn) -> Result<()> {
    unimplemented!()
}

pub fn read_request(id: RequestId, db: &DbConn) -> Result<Option<DeviceData>> {
    unimplemented!()
}

pub fn update_request(id: RequestId, data: RequestData, db: &DbConn) -> Result<()> {
    unimplemented!()
}

pub fn delete_request(id: RequestId, db: &DbConn) -> Result<()> {
    unimplemented!()
}

pub fn find_request_by_code(code: &Code, db: &DbConn) -> Result<()> {
    unimplemented!()
}
