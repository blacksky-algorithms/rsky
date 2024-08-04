use crate::account_manager::helpers::auth::{create_service_jwt, ServiceJwtParams};
use crate::auth_verifier::AccessFull;
use crate::models::{ErrorCode, ErrorMessageResponse};
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::GetServiceAuthOutput;
use secp256k1::SecretKey;
use std::env;

#[rocket::get("/xrpc/com.atproto.server.getServiceAuth?<aud>")]
pub async fn get_service_auth(
    aud: String,
    auth: AccessFull,
) -> Result<Json<GetServiceAuthOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    let did = auth.access.credentials.unwrap().did.unwrap();
    // We just use the repo signing key
    let private_key = env::var("PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX").unwrap();
    let keypair = SecretKey::from_slice(&hex::decode(private_key.as_bytes()).unwrap()).unwrap();
    match create_service_jwt(ServiceJwtParams {
        iss: did,
        aud,
        exp: None,
        keypair,
    })
    .await
    {
        Ok(token) => Ok(Json(GetServiceAuthOutput { token })),
        Err(error) => {
            eprintln!("Internal Error: {error}");
            let internal_error = ErrorMessageResponse {
                code: Some(ErrorCode::InternalServerError),
                message: Some("Internal error".to_string()),
            };
            return Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ));
        }
    }
}
