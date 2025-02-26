use rsky_pds::build_rocket;

#[rocket::main]
async fn main() {
    let subscriber = tracing_subscriber::FmtSubscriber::new();
    tracing::subscriber::set_global_default(subscriber).unwrap();
    let _ = build_rocket(None).await.launch().await;
}
