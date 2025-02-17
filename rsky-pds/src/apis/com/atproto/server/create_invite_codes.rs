use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::AdminToken;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::{
    AccountCodes, CreateInviteCodesInput, CreateInviteCodesOutput,
};
use crate::db::DbConn;

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.server.createInviteCodes",
    format = "json",
    data = "<body>"
)]
pub async fn create_invite_codes(
    body: Json<CreateInviteCodesInput>,
    _auth: AdminToken,
    db: &DbConn,
) -> Result<Json<CreateInviteCodesOutput>, ApiError> {
    // @TODO: verify admin auth token
    let CreateInviteCodesInput {
        use_count,
        code_count,
        for_accounts,
    } = body.into_inner();
    let for_accounts = for_accounts.unwrap_or_else(|| vec!["admin".to_owned()]);

    let mut account_codes: Vec<AccountCodes> = Vec::new();
    for account in for_accounts {
        let codes = super::gen_invite_codes(code_count);
        account_codes.push(AccountCodes { account, codes });
    }

    match AccountManager::create_invite_codes(account_codes.clone(), use_count, &db).await {
        Ok(_) => Ok(Json(CreateInviteCodesOutput {
            codes: account_codes,
        })),
        Err(error) => {
            tracing::error!("Internal Error: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
