use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::INVALID_HANDLE;
use anyhow::bail;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::{CreateSessionInput, CreateSessionOutput};

async fn inner_create_session(
    body: Json<CreateSessionInput>,
) -> Result<CreateSessionOutput, anyhow::Error> {
    let CreateSessionInput {
        password,
        identifier,
    } = body.into_inner();
    let identifier = identifier.to_lowercase();

    let user = match identifier.contains("@") {
        true => {
            AccountManager::get_account_by_email(
                &identifier,
                Some(AvailabilityFlags {
                    include_deactivated: Some(true),
                    include_taken_down: Some(true),
                }),
            )
            .await
        }
        false => {
            AccountManager::get_account(
                &identifier,
                Some(AvailabilityFlags {
                    include_deactivated: Some(true),
                    include_taken_down: Some(true),
                }),
            )
            .await
        }
    };
    if let Ok(Some(user)) = user {
        let mut app_password_name: Option<String> = None;
        let valid_account_pass =
            AccountManager::verify_account_password(&user.did, &password).await?;
        if !valid_account_pass {
            app_password_name = AccountManager::verify_app_password(&user.did, &password).await?;
            if app_password_name.is_none() {
                bail!("Invalid identifier or password")
            }
        }
        if user.takedown_ref.is_some() {
            bail!("Account has been taken down")
        }
        let (access_jwt, refresh_jwt) =
            AccountManager::create_session(user.did.clone(), app_password_name).await?;
        Ok(CreateSessionOutput {
            did: user.did,
            did_doc: None,
            handle: user.handle.unwrap_or(INVALID_HANDLE.to_string()),
            email: user.email,
            email_confirmed: Some(user.email_confirmed_at.is_some()),
            access_jwt,
            refresh_jwt,
        })
    } else {
        bail!("Invalid identifier or password")
    }
}

#[rocket::post(
    "/xrpc/com.atproto.server.createSession",
    format = "json",
    data = "<body>"
)]
pub async fn create_session(
    body: Json<CreateSessionInput>,
) -> Result<Json<CreateSessionOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    // @TODO: Add rate limiting

    match inner_create_session(body).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            eprintln!("{error:?}");
            let internal_error = ErrorMessageResponse {
                code: Some(ErrorCode::InternalServerError),
                message: Some(error.to_string()),
            };
            return Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ));
        }
    }
}
