use rsky_pds::build_rocket;
use rsky_pds::drain;

#[rocket::main]
async fn main() {
    let _ = &*rsky_pds::context::PDS_REPO_SIGNING_KEYPAIR;
    let _ = &*rsky_pds::account_manager::helpers::auth::PDS_JWT_SIGNER;
    let _ = &*rsky_pds::apis::com::atproto::server::PDS_PLC_ROTATION_KEYPAIR;

    let subscriber = tracing_subscriber::FmtSubscriber::new();
    tracing::subscriber::set_global_default(subscriber).unwrap();
    match drain::parse_args(std::env::args().skip(1)) {
        Ok(None) => {
            let _ = build_rocket(None).await.launch().await;
        }
        Ok(Some(args)) => {
            dotenvy::dotenv().ok();
            match drain::run_from_env(args).await {
                Ok(status) => {
                    println!("{}", serde_json::to_string_pretty(&status).unwrap());
                    std::process::exit(if status.fully_drained { 0 } else { 1 });
                }
                Err(err) => {
                    tracing::error!(?err, "drain failed");
                    std::process::exit(2);
                }
            }
        }
        Err(err) => {
            tracing::error!(%err, "invalid arguments");
            std::process::exit(2);
        }
    }
}
