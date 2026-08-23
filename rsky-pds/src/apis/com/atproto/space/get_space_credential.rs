use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::host::{
    def_app_access, def_policy, local_space_def, ActorJtiStore, FixedKeyResolver,
};
use crate::apis::com::atproto::space::{internal_error, parse_space_uri};
use crate::apis::ApiError;
use crate::config::ServerConfig;
use crate::space_auth::{
    now_secs, resolve_signing_did_key, verify_issuance_proof, ATPROTO_KEY_IDS,
};
use crate::SharedIdResolver;
use rocket::request::{FromRequest, Outcome, Request};
use rocket::serde::json::Json;
use rocket::State;
use rsky_common::get_random_str;
use rsky_lexicon::com::atproto::space::{GetSpaceCredentialInput, GetSpaceCredentialOutput};
use rsky_space::credential::decode;
use rsky_space_host::attestation::HttpMetadataFetcher;
use rsky_space_host::authority::Authority;
use rsky_space_host::error::HostError;
use rsky_space_host::signing::Signer;
use secp256k1::SecretKey;

/// The delegation token, presented as this request's bearer credential, and
/// the thumbprint of the DPoP key the minted credential binds to.
///
/// The thumbprint comes from the verified proof's own `jwk`, never from a
/// request field: a field would be an assertion anyone holding a delegation
/// token could make about a key someone else controls. The proof carries no
/// `ath` -- a delegation token is a grant, not a bound token -- and is checked
/// before the grant is consumed, so a caller with a bad proof does not burn
/// its single-use token finding out.
pub struct Issuance {
    pub delegation_token: String,
    pub dpop_jkt: String,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Issuance {
    type Error = ApiError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let dpop_jkt = match verify_issuance_proof(req).await {
            Ok(jkt) => jkt,
            Err(error) => {
                tracing::debug!(%error, "credential issuance proof rejected");
                let error = ApiError::AuthRequiredError(format!("InvalidDpopProof: {error}"));
                req.local_cache(|| Some(error.clone()));
                return Outcome::Error((rocket::http::Status::Unauthorized, error));
            }
        };
        match crate::auth_verifier::bearer_token_from_req(req) {
            Ok(Some(delegation_token)) => Outcome::Success(Issuance {
                delegation_token,
                dpop_jkt,
            }),
            _ => {
                let error = ApiError::AuthRequiredError("delegation token required".to_string());
                req.local_cache(|| Some(error.clone()));
                Outcome::Error((rocket::http::Status::Unauthorized, error))
            }
        }
    }
}

/// Exchange a delegation token (plus a client attestation when the space gates
/// on app identity) for a space credential (spec §Access control).
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.space.getSpaceCredential",
    format = "json",
    data = "<body>"
)]
pub async fn space_get_space_credential(
    body: Json<GetSpaceCredentialInput>,
    issuance: Issuance,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    server_config: &State<ServerConfig>,
    id_resolver: &State<SharedIdResolver>,
) -> Result<Json<GetSpaceCredentialOutput>, ApiError> {
    let GetSpaceCredentialInput {
        space,
        client_attestation,
    } = body.into_inner();
    let Issuance {
        delegation_token,
        dpop_jkt,
    } = issuance;
    let space_id = parse_space_uri(&space)?;
    let (def, space_store, keypair) =
        local_space_def(actor_store, blobstore_factory, &space_id).await?;

    // The delegation token is single-use: consume its jti up front.
    let decoded = decode(&delegation_token)
        .map_err(|error| ApiError::InvalidRequest(format!("bad delegation token: {error}")))?;
    let user_did = decoded.claims.iss.clone();
    let fresh = space_store
        .consume_jti(
            &decoded.claims.jti,
            decoded.claims.exp as i64,
            now_secs() as i64,
        )
        .await
        .map_err(internal_error("jti store failure"))?;
    if !fresh {
        return Err(ApiError::InvalidRequest(
            "delegation token replayed".to_string(),
        ));
    }
    let user_key = resolve_signing_did_key(actor_store, id_resolver, &user_did, ATPROTO_KEY_IDS)
        .await
        .map_err(|error| {
            ApiError::InvalidRequest(format!("could not resolve {user_did}: {error}"))
        })?;

    let signer = Signer::from_secret(
        SecretKey::from_slice(&keypair.secret_bytes()).expect("actor key is a valid secret"),
    );
    let policy = def_policy(
        &def,
        &space_store,
        signer.clone(),
        &space_id.authority,
        &server_config.identity.plc_url,
    )
    .await?;
    let authority = Authority::new(space_id.clone(), signer, def_app_access(&def));
    let credential = authority
        .get_space_credential(
            &delegation_token,
            client_attestation.as_deref(),
            &policy,
            &FixedKeyResolver(user_key),
            &HttpMetadataFetcher::new(),
            &ActorJtiStore(space_store),
            now_secs(),
            get_random_str(),
            &dpop_jkt,
        )
        .await
        .map_err(|error| match error {
            HostError::NotAuthorized => {
                ApiError::AuthRequiredError("user not authorized for this space".to_string())
            }
            HostError::ClientNotAuthorized => {
                ApiError::AuthRequiredError("client not authorized for this space".to_string())
            }
            HostError::AttestationRequired => ApiError::BadRequest(
                "AttestationRequired".to_string(),
                "this space requires a client attestation".to_string(),
            ),
            other => ApiError::InvalidRequest(other.to_string()),
        })?;
    Ok(Json(GetSpaceCredentialOutput { credential }))
}
