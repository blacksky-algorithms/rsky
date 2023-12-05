use std::env;
use rand::{distributions::Alphanumeric, Rng}; // 0.8

// Formatted xxxxx-xxxxx
pub fn get_random_token() -> String {
    let token: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();
    token[0..5].to_owned() + "-" + &token[5..10]
}

// generate an invite code preceded by the hostname
// with '.'s replaced by '-'s so it is not mistakable for a link
// ex: blacksky-app-abc234-567xy
// regex: blacksky-app-[a-z2-7]{5}-[a-z2-7]{5}
pub fn gen_invite_code() -> String {
    env::var("HOSTNAME").unwrap_or("blacksky.app".to_owned()).replace(".", "-") + "-" + &get_random_token()
}

pub fn gen_invite_codes(
    count: i32
) -> Vec<String> {
    let mut codes = Vec::new();
    for _i in 0..count {
        codes.push(gen_invite_code());
    }
    codes
}

pub mod confirm_email;
pub mod create_account;
pub mod create_app_password;
pub mod create_invite_code;
pub mod create_invite_codes;
pub mod create_session;
pub mod delete_account;
pub mod delete_session;
pub mod describe_server;
pub mod get_account_invite_codes;
pub mod get_session;
pub mod list_app_passwords;
pub mod refresh_session;
pub mod request_account_delete;
pub mod request_email_confirmation;
pub mod request_email_update;
pub mod request_password_reset;
pub mod reset_password;
pub mod revoke_app_password;
pub mod update_email;
pub mod reserve_signing_key;