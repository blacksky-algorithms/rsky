use crate::db::DbConn;
use anyhow::Result;
use rsky_common;
use rsky_oauth::oauth_provider::device::device_data::DeviceData;
use rsky_oauth::oauth_provider::request::code::Code;
use rsky_oauth::oauth_provider::request::request_data::RequestData;
use rsky_oauth::oauth_provider::request::request_id::RequestId;
use rsky_oauth::oauth_provider::request::request_store::{FoundRequestResult, UpdateRequestData};

pub fn create_qb(id: RequestId, data: RequestData, db: &DbConn) -> Result<()> {
    unimplemented!()
}

pub fn read_request(id: RequestId, db: &DbConn) -> Result<Option<DeviceData>> {
    unimplemented!()
}

pub fn update_qb(db: &DbConn, id: RequestId, data: UpdateRequestData) -> Result<()> {
    unimplemented!()
}

pub fn delete_request(id: RequestId, db: &DbConn) -> Result<()> {
    unimplemented!()
}

pub fn find_request_by_code(code: &Code, db: &DbConn) -> Result<()> {
    unimplemented!()
}

pub fn find_by_code_qb(db: &DbConn, code: Code) -> Result<()> {
    unimplemented!()
}

pub fn row_to_found_request_result() -> FoundRequestResult {
    unimplemented!()
}

pub fn remove_by_id_qb(db: &DbConn, id: RequestId) -> Result<()> {
    unimplemented!()
}
