use crate::account_manager::helpers::invite::CodeDetail;
use crate::account_manager::AccountManager;
use crate::apis::com::atproto::server::gen_invite_codes;
use crate::auth_verifier::AccessFull;
use crate::common::env::{env_bool, env_int};
use crate::common::RFC3339_VARIANT;
use crate::models::{ErrorCode, ErrorMessageResponse};
use anyhow::{bail, Result};
use chrono::NaiveDateTime;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::GetAccountInviteCodesOutput;
use std::time::SystemTime;

struct CalculateCodesToCreateOpts {
    pub user_created_at: usize,
    pub codes: Vec<CodeDetail>,
    pub epoch: usize,
    pub interval: usize,
}

/**
 * WARNING: TRICKY SUBTLE MATH - DON'T MESS WITH THIS FUNCTION UNLESS YOU'RE VERY CONFIDENT
 * if the user wishes to create available codes & the server allows that,
 * we determine the number to create by dividing their account lifetime by the interval at which they can create codes
 * if an invite epoch is provided, we only calculate available invites since that epoch
 * we allow a max of 5 open codes at a given time
 * note: even if a user is disabled from future invites, we still create the invites for bookkeeping, we just immediately disable them as well
 */
fn calculate_codes_to_create(opts: CalculateCodesToCreateOpts) -> Result<(usize, usize)> {
    // for the sake of generating routine interval codes, we do not count explicitly gifted admin codes
    let routine_codes: Vec<CodeDetail> = opts
        .codes
        .into_iter()
        .filter(|code| code.created_by != "admin")
        .collect();
    let unused_routine_codes: Vec<CodeDetail> = routine_codes
        .clone()
        .into_iter()
        .filter(|row| !row.disabled && row.available as usize > row.uses.len())
        .collect();

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("timestamp in micros since UNIX epoch")
        .as_micros() as usize;
    let user_lifespan = now - opts.user_created_at;

    // how many codes a user could create within the current epoch if they have 0
    let could_create: usize;

    if opts.user_created_at >= opts.epoch {
        // if the user was created after the epoch, then they can create a code for each interval since the epoch
        could_create = user_lifespan / opts.interval;
    } else {
        // if the user was created before the epoch, we:
        // - calculate the total intervals since account creation
        // - calculate the total intervals before the epoch
        // - subtract the two
        let could_create_total = user_lifespan / opts.interval;
        let user_pre_epoch_lifespan = opts.epoch - opts.user_created_at;
        let could_create_before_epoch = user_pre_epoch_lifespan / opts.interval;
        could_create = could_create_total - could_create_before_epoch;
    }
    // we count the codes that the user has created within the current epoch
    let epoch_codes: Vec<CodeDetail> = routine_codes
        .clone()
        .into_iter()
        .filter(|code| {
            let datetime = NaiveDateTime::parse_from_str(&code.created_at, RFC3339_VARIANT)
                .unwrap()
                .and_utc()
                .timestamp_micros() as usize;
            datetime > opts.epoch
        })
        .collect();
    // finally we subtract the number of codes they currently have from the number that they could
    // create, and take a max of 5
    let to_create = std::cmp::min(
        5 - unused_routine_codes.len(),
        could_create - epoch_codes.len(),
    );
    Ok((to_create, routine_codes.len() + to_create))
}

async fn inner_get_account_invite_codes(
    include_used: bool,
    create_available: bool,
    auth: AccessFull,
) -> Result<GetAccountInviteCodesOutput> {
    let requester = auth.access.credentials.unwrap().did.unwrap();
    let account = AccountManager::get_account(&requester, None).await?;
    let mut user_codes = AccountManager::get_account_invite_codes(&requester).await?;

    if let Some(account) = account {
        let mut created: Vec<CodeDetail> = Vec::new();
        if create_available
            && env_bool("PDS_INVITE_REQUIRED").unwrap_or(true)
            && env_int("PDS_INVITE_INTERVAL").is_some()
        {
            let user_created_at =
                NaiveDateTime::parse_from_str(&account.created_at, RFC3339_VARIANT)?
                    .and_utc()
                    .timestamp_micros() as usize;
            let (to_create, total) = calculate_codes_to_create(CalculateCodesToCreateOpts {
                user_created_at,
                codes: user_codes.clone(),
                epoch: env_int("PDS_INVITE_EPOCH").unwrap_or(0),
                interval: env_int("PDS_INVITE_INTERVAL").unwrap(),
            })?;
            if to_create > 0 {
                let codes = gen_invite_codes(to_create as i32);
                created = AccountManager::create_account_invite_codes(
                    &requester,
                    codes,
                    total,
                    account.invites_disabled.unwrap_or(0) == 1,
                )
                .await?;
            }
        }
        let mut all_codes: Vec<CodeDetail> = Vec::new();
        all_codes.append(&mut created);
        all_codes.append(&mut user_codes);
        let filtered: Vec<CodeDetail> = all_codes
            .into_iter()
            .filter(|code| {
                if code.disabled {
                    return false;
                };
                if !include_used && code.uses.len() >= code.available as usize {
                    return false;
                }
                true
            })
            .collect();
        Ok(GetAccountInviteCodesOutput { codes: filtered })
    } else {
        bail!("Account not found")
    }
}

#[allow(non_snake_case)]
#[allow(unused_variables)]
#[rocket::get("/xrpc/com.atproto.server.getAccountInviteCodes?<includeUsed>&<createAvailable>")]
pub async fn get_account_invite_codes(
    includeUsed: bool,
    createAvailable: bool,
    auth: AccessFull,
) -> Result<Json<GetAccountInviteCodesOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    match inner_get_account_invite_codes(includeUsed, createAvailable, auth).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            eprintln!("@LOG: ERROR: {error}");
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
