use rsky_pds::build_rocket;

#[rocket::main]
async fn main() {
    let _ = build_rocket().await.launch().await;
}
