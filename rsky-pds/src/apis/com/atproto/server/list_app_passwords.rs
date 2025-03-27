use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::AccessFull;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::{AppPassword, ListAppPasswordsOutput};

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.server.listAppPasswords")]
pub async fn list_app_passwords(
    auth: AccessFull,
    account_manager: AccountManager,
) -> Result<Json<ListAppPasswordsOutput>, ApiError> {
    let did = auth.access.credentials.unwrap().did.unwrap();
    match account_manager.list_app_passwords(&did).await {
        Ok(passwords) => {
            let passwords: Vec<AppPassword> = passwords
                .into_iter()
                .map(|password| AppPassword {
                    name: password.0,
                    created_at: password.1,
                })
                .collect();
            Ok(Json(ListAppPasswordsOutput { passwords }))
        }
        Err(error) => {
            tracing::error!("Internal Error: {error}");
            return Err(ApiError::RuntimeError);
        }
    }
}
