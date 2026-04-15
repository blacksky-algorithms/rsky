use rsky_pds::build_rocket;

#[rocket::main]
async fn main() {
    let _ = &*rsky_pds::context::PDS_REPO_SIGNING_KEYPAIR;
    let _ = &*rsky_pds::auth_verifier::PDS_JWT_KEYPAIR;
    let _ = &*rsky_pds::apis::com::atproto::server::PDS_PLC_ROTATION_KEYPAIR;

    let subscriber = tracing_subscriber::FmtSubscriber::new();
    tracing::subscriber::set_global_default(subscriber).unwrap();
    let _ = build_rocket(None).await.launch().await;
}
