use rocket::local::asynchronous::Client;
use rsky_pds::build_rocket;
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres;
use testcontainers_modules::postgres::Postgres;
use tokio::sync::OnceCell;

static CLIENT: OnceCell<Client> = OnceCell::const_new();
static POSTGRES: OnceCell<ContainerAsync<Postgres>> = OnceCell::const_new();

pub async fn setup() -> &'static Client {
    POSTGRES
        .get_or_init(|| async {
            postgres::Postgres::default()
                .start()
                .await
                .expect("Valid postgres instance")
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
