use rsky_pds::build_rocket;
use rsky_pds::cli;

#[rocket::main]
async fn main() {
    let _ = &*rsky_pds::context::PDS_REPO_SIGNING_KEYPAIR;
    let _ = &*rsky_pds::account_manager::helpers::auth::PDS_JWT_SIGNER;
    let _ = &*rsky_pds::apis::com::atproto::server::PDS_PLC_ROTATION_KEYPAIR;

    rsky_pds::logging::init(rsky_pds::logging::LogFormat::from_env());
    match cli::parse_args(std::env::args().skip(1)) {
        Ok(None) => {
            let _ = build_rocket(None).await.launch().await;
        }
        Ok(Some(command)) => {
            dotenvy::dotenv().ok();
            match cli::run(command).await {
                Ok((result, code)) => {
                    println!("{}", serde_json::to_string_pretty(&result).unwrap());
                    std::process::exit(code);
                }
                Err(err) => {
                    tracing::error!(?err, "maintenance command failed");
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
