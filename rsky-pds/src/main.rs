use rsky_pds::build_rocket;
use tracing_subscriber::fmt::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

#[rocket::main]
async fn main() {
    let _ = &*rsky_pds::context::PDS_REPO_SIGNING_KEYPAIR;
    let _ = &*rsky_pds::auth_verifier::PDS_JWT_KEYPAIR;
    let _ = &*rsky_pds::apis::com::atproto::server::PDS_PLC_ROTATION_KEYPAIR;

    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(Layer::new())
        .init();

    let _ = build_rocket(None).await.launch().await;
}
