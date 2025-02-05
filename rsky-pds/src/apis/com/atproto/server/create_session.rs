use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::{CreateSessionInput, CreateSessionOutput};
use rsky_syntax::handle::INVALID_HANDLE;

#[tracing::instrument(skip_all)]
async fn inner_create_session(
    body: Json<CreateSessionInput>,
) -> Result<CreateSessionOutput, ApiError> {
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
        let valid_account_pass;
        match AccountManager::verify_account_password(&user.did, &password).await {
            Ok(res) => {
                valid_account_pass = res;
            }
            Err(e) => {
                tracing::error!("{e:?}");
                return Err(ApiError::RuntimeError);
            }
        }
        if !valid_account_pass {
            match AccountManager::verify_app_password(&user.did, &password).await {
                Ok(res) => {
                    app_password_name = res;
                }
                Err(e) => {
                    tracing::error!("{e:?}");
                    return Err(ApiError::RuntimeError);
                }
            }
            if app_password_name.is_none() {
                return Err(ApiError::InvalidLogin);
            }
        }
        if user.takedown_ref.is_some() {
            return Err(ApiError::AccountTakendown);
        }
        let (access_jwt, refresh_jwt);
        match AccountManager::create_session(user.did.clone(), app_password_name).await {
            Ok(res) => {
                (access_jwt, refresh_jwt) = res;
            }
            Err(e) => {
                tracing::error!("{e:?}");
                return Err(ApiError::RuntimeError);
            }
        }
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
        Err(ApiError::InvalidLogin)
    }
}

#[rocket::post(
    "/xrpc/com.atproto.server.createSession",
    format = "json",
    data = "<body>"
)]
pub async fn create_session(
    body: Json<CreateSessionInput>,
) -> Result<Json<CreateSessionOutput>, ApiError> {
    // @TODO: Add rate limiting

    match inner_create_session(body).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => Err(error),
    }
}
