use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::{parse_space_uri, space_error};
use crate::apis::ApiError;
use crate::auth_verifier::bearer_token_from_req;
use crate::space_auth::verify_space_service_token;
use crate::SharedIdResolver;
use rocket::serde::json::Json;
use rocket::{Request, State};
use rsky_lexicon::com::atproto::space::NotifyCredentialRevokedInput;

pub const NOTIFY_CREDENTIAL_REVOKED_LXM: &str = "com.atproto.space.notifyCredentialRevoked";

pub struct BearerToken(String);

#[rocket::async_trait]
impl<'r> rocket::request::FromRequest<'r> for BearerToken {
    type Error = ApiError;

    async fn from_request(req: &'r Request<'_>) -> rocket::request::Outcome<Self, Self::Error> {
        match bearer_token_from_req(req) {
            Ok(Some(token)) => rocket::request::Outcome::Success(BearerToken(token)),
            _ => rocket::request::Outcome::Error((
                rocket::http::Status::Unauthorized,
                ApiError::AuthRequiredError("missing bearer token".to_string()),
            )),
        }
    }
}

/// `com.atproto.space.notifyCredentialRevoked`: service auth from the space
/// authority, addressed to the repo DID, recording `jti`s a repo host must
/// now reject (spec §Access control).
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.space.notifyCredentialRevoked",
    format = "json",
    data = "<body>"
)]
pub async fn space_notify_credential_revoked(
    body: Json<NotifyCredentialRevokedInput>,
    token: BearerToken,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    id_resolver: &State<SharedIdResolver>,
) -> Result<(), ApiError> {
    let NotifyCredentialRevokedInput { space, credentials } = body.into_inner();
    let space_id = parse_space_uri(&space)?;
    if credentials.is_empty() || credentials.len() > 100 {
        return Err(ApiError::InvalidRequest(
            "credentials must hold 1 to 100 jtis".to_string(),
        ));
    }
    let claims = verify_space_service_token(
        actor_store,
        id_resolver,
        &token.0,
        NOTIFY_CREDENTIAL_REVOKED_LXM,
        &space_id.authority,
    )
    .await?;
    if claims.iss != space_id.authority {
        return Err(ApiError::InvalidToken("Token is invalid".to_string()));
    }
    let repo_did = claims
        .aud
        .split('#')
        .next()
        .unwrap_or(&claims.aud)
        .to_string();
    if !actor_store.exists(&repo_did).await.unwrap_or(false) {
        return Err(ApiError::InvalidToken("Token is invalid".to_string()));
    }
    let reader = actor_store
        .read(
            repo_did.clone(),
            blobstore_factory.blobstore(repo_did.clone()),
        )
        .await
        .map_err(|error| ApiError::BadRequest("RepoNotFound".to_string(), error.to_string()))?;
    reader
        .space
        .revoke_credentials(&space_id.uri(), &credentials)
        .await
        .map_err(space_error)?;
    // A locally anchored space's revocation list also lives on the authority's
    // store so credential verification sees it regardless of the repo read.
    if space_id.authority != repo_did
        && actor_store
            .exists(&space_id.authority)
            .await
            .unwrap_or(false)
    {
        if let Ok(authority_reader) = actor_store
            .read(
                space_id.authority.clone(),
                blobstore_factory.blobstore(space_id.authority.clone()),
            )
            .await
        {
            if let Err(error) = authority_reader
                .space
                .revoke_credentials(&space_id.uri(), &credentials)
                .await
            {
                tracing::warn!(%error, "failed to record revocation on the authority's store");
            }
        }
    }
    Ok(())
}
