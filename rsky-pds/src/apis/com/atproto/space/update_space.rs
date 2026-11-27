use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::space::SpacePolicy;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::host::parse_policy;
use crate::apis::com::atproto::space::publish::{member_dids, reconcile_published};
use crate::apis::com::atproto::space::{parse_space_uri, require_manage, space_def_error};
use crate::apis::ApiError;
use crate::auth_verifier::AccessSpace;
use crate::config::ServerConfig;
use crate::space_scope::ManageOp;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::UpdateSpaceInput;

/// Replace a space's policy object wholesale. Same `$type` is a config
/// change; a different `$type` migrates the policy and discards the previous
/// policy's state, including the member list (spec §Space policy).
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.space.updateSpace",
    format = "json",
    data = "<body>"
)]
pub async fn space_update_space(
    body: Json<UpdateSpaceInput>,
    auth: AccessSpace,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    server_config: &State<ServerConfig>,
) -> Result<(), ApiError> {
    let UpdateSpaceInput { space, policy } = body.into_inner();
    let space_id = parse_space_uri(&space)?;
    let credentials = auth.access.credentials.expect("credentials populated");
    let did = credentials.did.clone().expect("did populated");
    require_manage(&credentials, &did, &space_id, ManageOp::Update)?;
    let policy = parse_policy(policy)?;
    let reader = actor_store
        .read(did.clone(), blobstore_factory.blobstore(did.clone()))
        .await
        .map_err(|error| ApiError::BadRequest("RepoNotFound".to_string(), error.to_string()))?;
    let mut def = reader
        .space
        .live_space_def(&space_id.uri())
        .await
        .map_err(space_def_error)?;
    // Capture the outgoing member list before a policy migration clears it:
    // its published member records have to come down too.
    let was_memberspace = matches!(def.policy, SpacePolicy::Memberspace(_));
    let dropped_members = if was_memberspace {
        member_dids(&reader.space, &space_id.uri()).await?
    } else {
        Vec::new()
    };
    if def.policy.type_name() != policy.type_name() {
        reader
            .space
            .clear_members(&space_id.uri())
            .await
            .map_err(space_def_error)?;
    }
    def.policy = policy.clone();
    reader
        .space
        .update_space_def(def)
        .await
        .map_err(space_def_error)?;
    reconcile_published(
        actor_store,
        blobstore_factory,
        server_config,
        &reader.space,
        &did,
        &space_id,
        &policy,
        if was_memberspace {
            dropped_members
        } else {
            Vec::new()
        },
    )
    .await?;
    Ok(())
}
