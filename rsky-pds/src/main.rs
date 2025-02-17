use rsky_pds::build_rocket;

#[rocket::main]
async fn main() {
    let _ = build_rocket(None).await.launch().await;
}
