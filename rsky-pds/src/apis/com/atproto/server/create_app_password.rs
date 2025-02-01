use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::AccessFull;
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
) -> Result<Json<CreateAppPasswordOutput>, ApiError> {
    let CreateAppPasswordInput { name } = body.into_inner();
    match AccountManager::create_app_password(auth.access.credentials.unwrap().did.unwrap(), name)
        .await
    {
        Ok(app_password) => Ok(Json(app_password)),
        Err(error) => {
            eprintln!("Internal Error: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
