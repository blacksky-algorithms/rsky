use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::host::{
    def_policy, local_space_def, ActorJtiStore, FixedKeyResolver,
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
use rsky_space::credential::{decode, Confirmation};
use rsky_space_host::attestation::HttpMetadataFetcher;
use rsky_space_host::authority::Authority;
use rsky_space_host::error::HostError;
use rsky_space_host::signing::Signer;
use secp256k1::SecretKey;

/// The delegation token, when presented as this request's bearer credential,
/// and the thumbprint of the DPoP key, when the request carries a proof.
///
/// Two bindings are accepted. The proposal's is unauthenticated: the body names
/// a P-256 `did:key` (`credentialKey`, or `credentialKid` in the proposal's
/// prose) and the credential binds to it as `cnf.kid`. The earlier one binds to
/// the DPoP key as `cnf.jkt`, and that thumbprint comes from the verified
/// proof's own `jwk`, never from a request field. A proof, when sent, carries
/// no `ath` -- a delegation token is a grant, not a bound token -- and is
/// checked before the grant is consumed, so a caller with a bad proof does not
/// burn its single-use token finding out.
pub struct Issuance {
    /// Absent under a policy that requires no user assertion (e.g. a
    /// `memberspace` with `publicRead`).
    pub delegation_token: Option<String>,
    /// Absent when the request carries no DPoP proof.
    pub dpop_jkt: Option<String>,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Issuance {
    type Error = ApiError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        if req.headers().get_one("dpop").is_none() {
            // The proposal's unauthenticated request: everything is in the body.
            return Outcome::Success(Issuance {
                delegation_token: None,
                dpop_jkt: None,
            });
        }
        let dpop_jkt = match verify_issuance_proof(req).await {
            Ok(jkt) => Some(jkt),
            Err(error) => {
                tracing::debug!(%error, "credential issuance proof rejected");
                let error = ApiError::AuthRequiredError(format!("InvalidDpopProof: {error}"));
                req.local_cache(|| Some(error.clone()));
                return Outcome::Error((rocket::http::Status::Unauthorized, error));
            }
        };
        let delegation_token = crate::auth_verifier::bearer_token_from_req(req)
            .ok()
            .flatten();
        Outcome::Success(Issuance {
            delegation_token,
            dpop_jkt,
        })
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
        credential_key,
        delegation_token: body_token,
        client_attestation,
    } = body.into_inner();
    let Issuance {
        delegation_token: bearer_token,
        dpop_jkt,
    } = issuance;
    // Settled before the delegation token is consumed, so a request that
    // names no usable key does not burn its single-use grant finding out. A
    // body key wins over a DPoP proof sent beside it.
    let binding = match (credential_key, dpop_jkt) {
        (Some(key), _) => {
            Confirmation::kid(&key).map_err(|error| ApiError::InvalidRequest(error.to_string()))?
        }
        (None, Some(jkt)) => Confirmation::jkt(jkt),
        (None, None) => {
            return Err(ApiError::InvalidRequest(
                "credentialKey is required without a DPoP proof".to_string(),
            ))
        }
    };
    let space_id = parse_space_uri(&space)?;
    let (def, space_store, keypair) =
        local_space_def(actor_store, blobstore_factory, &space_id).await?;

    // The body field wins over the bearer credential (spec §Requesting a
    // space credential); a `publicRead` space requires neither.
    let delegation_token = body_token.or(bearer_token);
    let user_key = match &delegation_token {
        Some(token) => {
            // The delegation token is single-use: consume its jti up front.
            let decoded = decode(token).map_err(|error| {
                ApiError::InvalidRequest(format!("bad delegation token: {error}"))
            })?;
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
            let user_did = decoded.claims.iss.clone();
            Some(
                resolve_signing_did_key(actor_store, id_resolver, &user_did, ATPROTO_KEY_IDS)
                    .await
                    .map_err(|error| {
                        ApiError::InvalidRequest(format!("could not resolve {user_did}: {error}"))
                    })?,
            )
        }
        None => None,
    };

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
    let authority = Authority::new(space_id.clone(), signer);
    let credential = authority
        .get_space_credential(
            delegation_token.as_deref(),
            client_attestation.as_deref(),
            &policy,
            &FixedKeyResolver(user_key.unwrap_or_default()),
            &HttpMetadataFetcher::new(),
            &ActorJtiStore(space_store),
            now_secs(),
            get_random_str(),
            &binding,
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
