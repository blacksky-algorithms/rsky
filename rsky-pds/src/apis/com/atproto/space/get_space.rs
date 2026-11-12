use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::host::{def_to_policy, local_space_def};
use crate::apis::com::atproto::space::parse_space_uri;
use crate::apis::ApiError;
use crate::space_auth::{session_permits, SpaceReadAuth};
use crate::space_scope::SpaceRequest;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::GetSpaceOutput;

/// Describe a space anchored on a local account (space-host role). Accepts an
/// OAuth `read_self` grant or a space credential (spec §XRPC API).
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.space.getSpace?<space>")]
pub async fn space_get_space(
    space: String,
    auth: SpaceReadAuth,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
) -> Result<Json<GetSpaceOutput>, ApiError> {
    let space_id = parse_space_uri(&space)?;
    match &auth {
        SpaceReadAuth::Credential(credential) => {
            if credential.space_uri != space_id.uri() {
                return Err(ApiError::InvalidToken("Token is invalid".to_string()));
            }
        }
        SpaceReadAuth::Session { did, credentials } => {
            if !session_permits(
                credentials,
                did,
                &space_id,
                &SpaceRequest::ReadSelf { collection: None },
            ) {
                return Err(ApiError::AuthRequiredError(
                    "session does not cover reading this space".to_string(),
                ));
            }
        }
    }
    let (def, _, _) = local_space_def(actor_store, blobstore_factory, &space_id).await?;
    // `new` also sets `uri`, for clients of the spaces specification before
    // the field was renamed `space`; see `GetSpaceOutput::uri`.
    Ok(Json(GetSpaceOutput::new(
        space_id.uri(),
        def_to_policy(&def),
    )))
}
