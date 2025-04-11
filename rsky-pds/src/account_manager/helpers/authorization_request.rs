use crate::db::DbConn;
use crate::models::models;
use crate::models::models::AuthorizationRequest;
use crate::schema::pds::authorization_request::dsl as RequestSchema;
use anyhow::Result;
use diesel::row::NamedRow;
use diesel::*;
use diesel::{delete, insert_into, QueryDsl, RunQueryDsl, SelectableHelper};
use rsky_common;
use rsky_oauth::oauth_provider::now_as_secs;
use rsky_oauth::oauth_provider::request::code::Code;
use rsky_oauth::oauth_provider::request::request_data::RequestData;
use rsky_oauth::oauth_provider::request::request_id::RequestId;
use rsky_oauth::oauth_provider::request::request_store::{FoundRequestResult, UpdateRequestData};

pub fn row_to_request_data(request: models::AuthorizationRequest) -> RequestData {
    unimplemented!()
    // RequestData {
    //     client_id: request.client_id,
    //     client_auth: request.client_auth,
    //     parameters: request.parameters,
    //     expires_at: request.expires_at,
    //     device_id: request.device_id,
    //     sub: request.did,
    //     code: request.code,
    // }
}

pub fn row_to_found_request_result(row: AuthorizationRequest) -> FoundRequestResult {
    unimplemented!()
    // FoundRequestResult {
    //     id: row.id,
    //     data: RequestData {},
    // }
}

fn request_data_to_row(id: RequestId, data: RequestData) -> AuthorizationRequest {
    let id = id.into_inner();
    let did = match data.sub {
        None => None,
        Some(did) => Some(did.get()),
    };
    let device_id = match data.device_id {
        None => None,
        Some(device_id) => Some(device_id.into_inner()),
    };
    let client_id = data.client_id.into_inner();
    let code = match data.code {
        None => None,
        Some(code) => Some(code.val()),
    };
    AuthorizationRequest {
        id,
        did,
        device_id,
        client_id,
        client_auth: "".to_string(),
        parameters: "".to_string(),
        expires_at: 0,
        code,
    }
}

pub async fn create_qb(id: RequestId, data: RequestData, db: &DbConn) -> Result<()> {
    let value = request_data_to_row(id, data);
    db.run(move |conn| {
        let rows: Vec<AuthorizationRequest> = vec![value];
        insert_into(RequestSchema::authorization_request)
            .values(&rows)
            .execute(conn)
    })
    .await?;
    Ok(())
}

pub async fn read_qb(id: RequestId, db: &DbConn) -> Result<Option<AuthorizationRequest>> {
    let id = id.into_inner();
    let result = db
        .run(move |conn| {
            RequestSchema::authorization_request
                .filter(RequestSchema::id.eq(id))
                .select(models::AuthorizationRequest::as_select())
                .first(conn)
                .optional()
        })
        .await?;
    Ok(result)
}

pub async fn update_qb(id: RequestId, data: UpdateRequestData, db: &DbConn) -> Result<()> {
    let id = id.into_inner();
    db.run(move |conn| {
        if let Some(code) = data.code {
            update(RequestSchema::authorization_request)
                .filter(RequestSchema::id.eq(&id))
                .set((RequestSchema::code.eq(code.val()),))
                .execute(conn)?;
        }
        if let Some(sub) = data.sub {
            update(RequestSchema::authorization_request)
                .filter(RequestSchema::id.eq(&id))
                .set((RequestSchema::did.eq(sub.get()),))
                .execute(conn)?;
        }
        if let Some(device_id) = data.device_id {
            update(RequestSchema::authorization_request)
                .filter(RequestSchema::id.eq(&id))
                .set((RequestSchema::deviceId.eq(device_id.into_inner()),))
                .execute(conn)?;
        }
        if let Some(expires_at) = data.expires_at {
            let expires_at = expires_at as i64;
            update(RequestSchema::authorization_request)
                .filter(RequestSchema::id.eq(&id))
                .set((RequestSchema::expiresAt.eq(expires_at),))
                .execute(conn)?;
        }
        RequestSchema::authorization_request
            .filter(RequestSchema::id.eq(id))
            .select(models::AuthorizationRequest::as_select())
            .first(conn)
            .optional()
    })
    .await?;
    Ok(())
}

pub async fn remove_old_expired_qb(delay: Option<i64>, db: &DbConn) {
    // We allow some delay for the expiration time so that expired requests
    // can still be returned to the OAuthProvider library for error handling.
    let delay = delay.unwrap_or(600000);
    let expire_time = now_as_secs() as i64 - delay;

    db.run(move |conn| {
        delete(RequestSchema::authorization_request)
            .filter(RequestSchema::expiresAt.lt(expire_time))
            .execute(conn)
    })
    .await
    .unwrap();
}

pub async fn remove_by_id_qb(id: RequestId, db: &DbConn) -> Result<()> {
    let id = id.into_inner();
    db.run(move |conn| {
        delete(RequestSchema::authorization_request)
            .filter(RequestSchema::id.eq(id))
            .execute(conn)
    })
    .await?;

    Ok(())
}

pub async fn find_by_code_qb(db: &DbConn, code: Code) -> Result<Option<AuthorizationRequest>> {
    let code = code.val();
    let result = db
        .run(move |conn| {
            RequestSchema::authorization_request
                .filter(RequestSchema::code.eq(code))
                .select(models::AuthorizationRequest::as_select())
                .first(conn)
        })
        .await?;
    Ok(Some(result))
}
