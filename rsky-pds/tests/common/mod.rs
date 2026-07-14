use http_auth_basic::Credentials;
use rocket::http::{ContentType, Header};
use rocket::local::asynchronous::Client;
use rocket::serde::json::json;
use rsky_common::env::env_str;
use rsky_lexicon::com::atproto::server::CreateInviteCodeOutput;
use rsky_pds::config::{ServerConfig, ServiceDbConfig};
use rsky_pds::{build_rocket, RocketConfig};
use std::sync::Once;
use tempfile::TempDir;

static INIT_ENV: Once = Once::new();

/// Provides the environment the server expects when the caller (e.g. a
/// local `cargo test` run) hasn't configured it. CI sets its own values.
fn init_env() {
    INIT_ENV.call_once(|| {
        let defaults = [
            ("PDS_HOSTNAME", "rsky.com"),
            ("PDS_SERVICE_DID", "did:web:localho.st"),
            ("PDS_SERVICE_HANDLE_DOMAINS", ".rsky.com"),
            ("PDS_ADMIN_PASS", "3ed1c7b568d3328c44430add531a099f"),
            (
                "PDS_JWT_KEY_K256_PRIVATE_KEY_HEX",
                "9d5907143471e8f0e8df0f8b9512a8c5377878ee767f18fcf961055ecfc071cd",
            ),
            (
                "PDS_PLC_ROTATION_KEY_K256_PRIVATE_KEY_HEX",
                "fb478b39dd2ddf84bef135dd60f90381903eefadbb9df4b18a2b9b174ae72582",
            ),
            (
                "PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX",
                "71cfcf4882a6cff494c3d0affadd3858eb3a5838e7b5e15170e696a590a4fa01",
            ),
        ];
        for (key, value) in defaults {
            if std::env::var(key).is_err() {
                std::env::set_var(key, value);
            }
        }
    });
}

/**
    Fetch PDS_ADMIN_PASS to be used for creating initial accounts
*/
pub fn get_admin_token() -> String {
    let credentials = Credentials::new("admin", env_str("PDS_ADMIN_PASS").unwrap().as_str());
    credentials.as_http_header()
}

/**
    Start a client for the rsky-pds rocket instance backed by sqlite
    databases under a fresh temporary directory
*/
pub async fn get_client() -> (TempDir, Client) {
    init_env();
    let dir = tempfile::tempdir().expect("Valid temporary directory");
    let path = |name: &str| dir.path().join(name).to_str().unwrap().to_owned();
    let rocket_cfg = RocketConfig {
        service_db: Some(ServiceDbConfig {
            account_db_location: path("account.sqlite"),
            sequencer_db_location: path("sequencer.sqlite"),
            did_cache_db_location: path("did_cache.sqlite"),
        }),
        actor_store_directory: Some(path("actors")),
    };
    let client = Client::untracked(build_rocket(Some(rocket_cfg)).await)
        .await
        .expect("Valid Rocket instance");
    (dir, client)
}

/**
    Creates a mock account for testing purposes
*/
pub async fn create_account(client: &Client) -> (String, String) {
    let domain = client
        .rocket()
        .state::<ServerConfig>()
        .unwrap()
        .identity
        .service_handle_domains
        .first()
        .unwrap();
    let input = json!({
        "useCount": 1
    });

    let response = client
        .post("/xrpc/com.atproto.server.createInviteCode")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", get_admin_token()))
        .body(input.to_string())
        .dispatch()
        .await;
    let invite_code = response
        .into_json::<CreateInviteCodeOutput>()
        .await
        .unwrap()
        .code;

    let account_input = json!({
        "did": "did:plc:khvyd3oiw46vif5gm7hijslk",
        "email": "foo@example.com",
        "handle": format!("foo{domain}"),
        "password": "password",
        "inviteCode": invite_code
    });

    client
        .post("/xrpc/com.atproto.server.createAccount")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", get_admin_token()))
        .body(account_input.to_string())
        .dispatch()
        .await;

    ("foo@example.com".to_string(), "password".to_string())
}
