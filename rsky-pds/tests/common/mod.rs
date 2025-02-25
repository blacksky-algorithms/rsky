use anyhow::Result;
use diesel::{Connection, PgConnection};
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use dotenvy::dotenv;
use http_auth_basic::Credentials;
use rocket::http::{ContentType, Header, Status};
use rocket::local::asynchronous::Client;
use rocket::serde::json::json;
use rsky_common::env::env_str;
use rsky_lexicon::com::atproto::server::CreateInviteCodeOutput;
use rsky_pds::{build_rocket, RocketConfig};
use std::env;
use testcontainers::core::IntoContainerPort;
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, Image, ImageExt};
use testcontainers_modules::postgres;
use testcontainers_modules::postgres::Postgres;
use tokio::sync::OnceCell;

const MIGRATIONS: EmbeddedMigrations = embed_migrations!();

#[tracing::instrument(skip_all)]
pub fn establish_connection(database_url: &str) -> Result<PgConnection> {
    tracing::debug!("Establishing database connection");
    let result = PgConnection::establish(database_url).map_err(|error| {
        let context = format!("Error connecting to {database_url:?}");
        anyhow::Error::new(error).context(context)
    })?;

    Ok(result)
}

pub fn get_admin_token() -> String {
    let credentials = Credentials::new("admin", env_str("PDS_ADMIN_PASS").unwrap().as_str());
    credentials.as_http_header()
}

pub async fn get_postgres() -> ContainerAsync<Postgres> {
    let postgres = postgres::Postgres::default()
        .start()
        .await
        .expect("Valid postgres instance");
    let ip_address = postgres
        .get_bridge_ip_address()
        .await
        .expect("get bridged Ip")
        .to_string();
    let port = postgres.get_host_port_ipv4(5432).await.unwrap();
    let connection_string = format!("postgres://postgres:postgres@localhost:{port}/postgres",);
    let mut conn =
        establish_connection(connection_string.as_str()).expect("Connection  Established");
    conn.run_pending_migrations(MIGRATIONS).unwrap();
    postgres
}

pub async fn get_client(postgres: &ContainerAsync<Postgres>) -> Client {
    let ip_address = postgres.get_bridge_ip_address().await.unwrap().to_string();
    let port = postgres.get_host_port_ipv4(5432).await.unwrap();
    let connection_string = format!("postgres://postgres:postgres@localhost:{port}/postgres",);
    Client::untracked(
        build_rocket(Some(RocketConfig {
            db_url: String::from(connection_string),
        }))
        .await,
    )
    .await
    .expect("Valid Rocket instance")
}

pub async fn create_account(client: &Client) -> (String, String) {
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
        "email": "dummyemail@rsky.com",
        "handle": "dummaccount.rsky.com",
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

    ("dummyemail@rsky.com".to_string(), "password".to_string())
}
