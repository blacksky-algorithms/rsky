use rocket::http::{ContentType, Status};
use rocket::local::asynchronous::Client;
use serde_json::Value;
use std::ffi::OsString;
use std::sync::Mutex;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;

use rsky_pds::{RocketConfig, build_rocket};

mod common;

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvVarGuard {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(key);
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => unsafe { std::env::set_var(self.key, value) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}

async fn get_metadata_client(postgres: &ContainerAsync<Postgres>) -> Client {
    unsafe {
        std::env::set_var("PDS_ADMIN_PASSWORD", "test-admin-password");
        std::env::set_var("PDS_ADMIN_PASS", "test-admin-password");
        std::env::set_var("PDS_HOSTNAME", "pds.divine.video");
        std::env::set_var("PDS_SERVICE_DID", "did:web:pds.divine.video");
        std::env::set_var("PDS_SERVICE_HANDLE_DOMAINS", ".divine.video");
        std::env::set_var(
            "PDS_OAUTH_AUTHORIZATION_SERVER",
            "https://entryway.divine.video",
        );
        std::env::set_var(
            "PDS_JWT_KEY_K256_PRIVATE_KEY_HEX",
            "8f2a55949068468ad5d670dfd0c0a33d5b9e7e1a2c0d2059f0f8f8779d4d078d",
        );
        std::env::set_var(
            "PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX",
            "4f3edf983ac636a65a842ce7c78d9aa706d3b113bce036f4aeb4f7f7a5c5f3cf",
        );
        std::env::set_var(
            "PDS_PLC_ROTATION_KEY_K256_PRIVATE_KEY_HEX",
            "6c3699283bda56ad74f6b855546325b68d482e983852a5b0d1f5b0f8d7e79b4f",
        );
    }

    let port = postgres.get_host_port_ipv4(5432).await.unwrap();
    let connection_string = format!("postgres://postgres:postgres@localhost:{port}/postgres");
    Client::untracked(
        build_rocket(Some(RocketConfig {
            db_url: connection_string,
        }))
        .await,
    )
    .await
    .expect("Valid Rocket instance")
}

#[tokio::test]
async fn oauth_metadata_returns_protected_resource_metadata() {
    let _guard = ENV_LOCK.lock().unwrap();
    let _env_guard = EnvVarGuard::set(
        "PDS_OAUTH_AUTHORIZATION_SERVER",
        "https://entryway.divine.video",
    );

    let postgres = common::get_postgres().await;
    let client = get_metadata_client(&postgres).await;

    let response = client
        .get("/.well-known/oauth-protected-resource")
        .dispatch()
        .await;

    assert_eq!(response.status(), Status::Ok);

    let content_type = response
        .headers()
        .get_one("Content-Type")
        .expect("content type header");
    assert_eq!(content_type, ContentType::JSON.to_string());
    assert!(response.headers().get_one("Location").is_none());

    let body = response.into_json::<Value>().await.expect("json body");
    assert_eq!(
        body,
        serde_json::json!({
            "resource": "https://pds.divine.video",
            "authorization_servers": ["https://entryway.divine.video"]
        })
    );
}
