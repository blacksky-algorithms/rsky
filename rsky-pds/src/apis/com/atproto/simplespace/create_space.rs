use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::space::SpaceDefRow;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::simplespace::{merge_config, require_manage, space_error};
use crate::apis::com::atproto::space::host::{APP_ACCESS_OPEN, POLICY_MEMBER_LIST};
use crate::apis::com::atproto::space::{valid_key_part, valid_nsid};
use crate::apis::ApiError;
use crate::auth_verifier::AccessFull;
use crate::space_scope::ManageOp;
use rocket::serde::json::Json;
use rocket::State;
use rsky_common::tid::TID;
use rsky_lexicon::com::atproto::simplespace::{CreateSpaceInput, CreateSpaceOutput};
use rsky_space::space_id::SpaceId;

/// Create a space anchored on the caller's DID: the caller becomes the
/// authority. Defaults: `member-list` policy, `open` app access.
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.simplespace.createSpace",
    format = "json",
    data = "<body>"
)]
pub async fn simplespace_create_space(
    body: Json<CreateSpaceInput>,
    auth: AccessFull,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
) -> Result<Json<CreateSpaceOutput>, ApiError> {
    let CreateSpaceInput {
        space_type,
        skey,
        config,
    } = body.into_inner();
    let credentials = auth.access.credentials.expect("credentials populated");
    let did = credentials.did.clone().expect("did populated");
    if !valid_nsid(&space_type) {
        return Err(ApiError::InvalidRequest(format!(
            "invalid space type: {space_type}"
        )));
    }
    let skey = match skey {
        Some(skey) if valid_key_part(&skey, 512) => skey,
        Some(skey) => return Err(ApiError::InvalidRequest(format!("invalid skey: {skey}"))),
        None => TID::next_str(None).map_err(|_| ApiError::RuntimeError)?,
    };
    let space = SpaceId::new(did.clone(), space_type.clone(), skey.clone());
    require_manage(&credentials, &did, &space, ManageOp::Create)?;

    let mut def = SpaceDefRow {
        space_uri: space.uri(),
        space_type,
        skey,
        policy: POLICY_MEMBER_LIST.to_string(),
        app_access: APP_ACCESS_OPEN.to_string(),
        allowed_clients: None,
        managing_app: None,
        deleted: false,
    };
    if let Some(ref config) = config {
        def = merge_config(def, config)?;
    }
    let reader = actor_store
        .read(did.clone(), blobstore_factory.blobstore(did.clone()))
        .await
        .map_err(|error| ApiError::BadRequest("RepoNotFound".to_string(), error.to_string()))?;
    reader
        .space
        .create_space_def(def)
        .await
        .map_err(space_error)?;
    Ok(Json(CreateSpaceOutput { space: space.uri() }))
}
