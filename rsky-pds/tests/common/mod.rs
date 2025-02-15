use anyhow::Result;
use diesel::{Connection, PgConnection};
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use dotenvy::dotenv;
use http_auth_basic::Credentials;
use rocket::local::asynchronous::Client;
use rsky_common::env::env_str;
use rsky_pds::build_rocket;
use std::env;
use testcontainers::core::IntoContainerPort;
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::postgres;
use testcontainers_modules::postgres::Postgres;
use tokio::sync::OnceCell;

static CLIENT: OnceCell<Client> = OnceCell::const_new();
static POSTGRES: OnceCell<ContainerAsync<Postgres>> = OnceCell::const_new();

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

pub async fn setup() -> &'static Client {
    POSTGRES
        .get_or_init(|| async {
            let postgres = postgres::Postgres::default()
                .with_mapped_port(5432, 5432.tcp())
                .start()
                .await
                .expect("Valid postgres instance");
            let host_port = postgres.get_host_port_ipv4(5432).await.unwrap();
            let connection_string =
                format!("postgres://postgres:postgres@127.0.0.1:{host_port}/postgres",);
            let mut conn = establish_connection(connection_string.as_str()).unwrap();
            conn.run_pending_migrations(MIGRATIONS).unwrap();
            postgres
        })
        .await;
    CLIENT
        .get_or_init(|| async {
            Client::untracked(build_rocket().await)
                .await
                .expect("Valid Rocket instance")
        })
        .await
}

pub fn get_admin_token() -> String {
    let credentials = Credentials::new("admin", env_str("PDS_ADMIN_PASS").unwrap().as_str());
    credentials.as_http_header()
}
