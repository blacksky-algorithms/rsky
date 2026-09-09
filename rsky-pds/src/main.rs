use rsky_pds::build_rocket;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

#[rocket::main]
async fn main() {
    let _ = &*rsky_pds::context::PDS_REPO_SIGNING_KEYPAIR;
    let _ = &*rsky_pds::auth_verifier::PDS_JWT_KEYPAIR;
    let _ = &*rsky_pds::apis::com::atproto::server::PDS_PLC_ROTATION_KEYPAIR;

    // Only set up an OTLP exporter (and pay its cost) when a collector
    // endpoint is actually configured; see rsky_pds::telemetry.
    let otel_layer = rsky_pds::telemetry::layer();

    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(rsky_pds::telemetry::fmt_layer())
        .with(otel_layer)
        .init();

    let _ = build_rocket(None).await.launch().await;

    rsky_pds::telemetry::shutdown();
}
