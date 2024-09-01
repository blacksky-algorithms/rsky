use crate::account_manager::AccountManager;
use crate::auth_verifier::AccessFull;
use crate::models::{ErrorCode, ErrorMessageResponse};
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::{CreateAppPasswordInput, CreateAppPasswordOutput};

#[rocket::post(
    "/xrpc/com.atproto.server.createAppPassword",
    format = "json",
    data = "<body>"
)]
pub async fn create_app_password(
    body: Json<CreateAppPasswordInput>,
    auth: AccessFull,
) -> Result<Json<CreateAppPasswordOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    let CreateAppPasswordInput { name } = body.into_inner();
    match AccountManager::create_app_password(auth.access.credentials.unwrap().did.unwrap(), name)
        .await
    {
        Ok(app_password) => Ok(Json(app_password)),
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
