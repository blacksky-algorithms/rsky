use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::apis::com::atproto::server::PDS_PLC_ROTATION_KEYPAIR;
use crate::apis::ApiError;
use crate::auth_verifier::AccessStandard;
use crate::config::ServerConfig;
use crate::context::PDS_REPO_SIGNING_KEYPAIR;
use rocket::serde::json::Json;
use rocket::State;
use rsky_crypto::utils::encode_did_key;
use rsky_lexicon::com::atproto::identity::GetRecommendedDidCredentialsResponse;
use serde_json::json;

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.identity.getRecommendedDidCredentials")]
pub async fn get_recommended_did_credentials(
    auth: AccessStandard,
    cfg: &State<ServerConfig>,
    account_manager: AccountManager,
) -> Result<Json<GetRecommendedDidCredentialsResponse>, ApiError> {
    let requester = auth.access.credentials.unwrap().did.unwrap();
    let availability_flags = AvailabilityFlags {
        include_taken_down: Some(true),
        include_deactivated: Some(true),
    };
    let account = account_manager
        .get_account(&requester, Some(availability_flags))
        .await?
        .expect("Account not found despite valid access");

    let mut also_known_as = Vec::new();
    match account.handle {
        None => {}
        Some(res) => {
            also_known_as.push("at://".to_string() + res.as_str());
        }
    }

    let signing_key = encode_did_key(&PDS_REPO_SIGNING_KEYPAIR.public_key());
    let verification_methods = json!({
        "atproto": signing_key
    });

    let rotation_key = encode_did_key(&PDS_PLC_ROTATION_KEYPAIR.public_key());
    let rotation_keys = vec![rotation_key];

    let services = json!({
        "atproto_pds": {
            "type": "AtprotoPersonalDataServer",
            "endpoint": cfg.service.public_url
        }
    });
    let response = GetRecommendedDidCredentialsResponse {
        also_known_as,
        verification_methods,
        rotation_keys,
        services,
    };
    Ok(Json(response))
}
