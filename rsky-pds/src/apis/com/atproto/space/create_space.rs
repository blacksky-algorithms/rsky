use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::space::SpaceDefRow;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::server::PDS_PLC_ROTATION_KEYPAIR;
use crate::apis::com::atproto::space::host::parse_policy;
use crate::apis::com::atproto::space::kem::ensure_kem_key_published;
use crate::apis::com::atproto::space::publish::reconcile_published;
use crate::apis::com::atproto::space::{require_manage, space_def_error, valid_key_part};
use crate::apis::ApiError;
use crate::auth_verifier::AccessSpace;
use crate::config::ServerConfig;
use crate::space_scope::ManageOp;
use rocket::serde::json::Json;
use rocket::State;
use rsky_common::tid::TID;
use rsky_lexicon::com::atproto::space::{CreateSpaceInput, CreateSpaceOutput};
use rsky_space::space_id::SpaceId;
use rsky_syntax::nsid::ensure_valid_nsid;

/// Create a space with a given policy: the caller becomes the authority.
/// Unknown policy `$type`s are rejected as `UnsupportedPolicy` (spec §Space
/// policy).
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.space.createSpace",
    format = "json",
    data = "<body>"
)]
pub async fn space_create_space(
    body: Json<CreateSpaceInput>,
    auth: AccessSpace,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    server_config: &State<ServerConfig>,
) -> Result<Json<CreateSpaceOutput>, ApiError> {
    let CreateSpaceInput {
        space_type,
        skey,
        policy,
    } = body.into_inner();
    let credentials = auth.access.credentials.expect("credentials populated");
    let did = credentials.did.clone().expect("did populated");
    if ensure_valid_nsid(&space_type).is_err() {
        return Err(ApiError::InvalidRequest(format!(
            "invalid space type: {space_type}"
        )));
    }
    let policy = parse_policy(policy)?;
    let skey = match skey {
        Some(skey) if valid_key_part(&skey, 512) => skey,
        Some(skey) => return Err(ApiError::InvalidRequest(format!("invalid skey: {skey}"))),
        None => TID::next_str(None).map_err(|_| ApiError::RuntimeError)?,
    };
    let space = SpaceId::new(did.clone(), space_type.clone(), skey.clone());
    require_manage(&credentials, &did, &space, ManageOp::Create)?;

    let def = SpaceDefRow {
        space_uri: space.uri(),
        space_type,
        skey,
        policy: policy.clone(),
        deleted: false,
    };
    let reader = actor_store
        .read(did.clone(), blobstore_factory.blobstore(did.clone()))
        .await
        .map_err(|error| ApiError::BadRequest("RepoNotFound".to_string(), error.to_string()))?;
    reader
        .space
        .create_space_def(def)
        .await
        .map_err(space_def_error)?;
    // The authority holds a repo in every space it creates, so it publishes
    // its `#atproto_kem` key (proposal §KEM key).
    ensure_kem_key_published(
        actor_store,
        &server_config.identity.plc_url,
        &PDS_PLC_ROTATION_KEYPAIR.secret_key(),
        &did,
    )
    .await;
    // The creator is not made a member: the authority reads and writes its
    // own space under every policy without one (`Policy::authorizes`), and is
    // never on its own member list, as the 2026-10-29 reference's
    // `createSpace` and upstream `atproto-pds` leave it. Seeding it put the
    // authority in `listMembers` and, under `publishMembers`, published it as
    // a member of its own space.
    reconcile_published(
        actor_store,
        blobstore_factory,
        server_config,
        &reader.space,
        &did,
        &space,
        &policy,
        Vec::new(),
    )
    .await?;
    Ok(Json(CreateSpaceOutput { uri: space.uri() }))
}
