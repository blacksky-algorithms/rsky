use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::space::SpaceWrite;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::{
    apply_space_writes, commit_meta, parse_space_uri, valid_key_part, valid_nsid,
};
use crate::apis::ApiError;
use crate::auth_verifier::AccessFull;
use crate::config::ServerConfig;
use crate::space_auth::session_permits;
use crate::space_scope::{SpaceAction, SpaceRequest};
use rocket::serde::json::Json;
use rocket::State;
use rsky_common::tid::TID;
use rsky_lexicon::com::atproto::space::{CreateRecordInput, CreateRecordOutput};

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.space.createRecord",
    format = "json",
    data = "<body>"
)]
pub async fn space_create_record(
    body: Json<CreateRecordInput>,
    auth: AccessFull,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    server_config: &State<ServerConfig>,
) -> Result<Json<CreateRecordOutput>, ApiError> {
    let CreateRecordInput {
        space,
        collection,
        rkey,
        validate: _,
        record,
    } = body.into_inner();
    let space_id = parse_space_uri(&space)?;
    let credentials = auth.access.credentials.expect("credentials populated");
    let did = credentials.did.clone().expect("did populated");
    if !valid_nsid(&collection) {
        return Err(ApiError::InvalidRequest(format!(
            "invalid collection: {collection}"
        )));
    }
    let rkey = match rkey {
        Some(rkey) if valid_key_part(&rkey, 512) => rkey,
        Some(rkey) => return Err(ApiError::InvalidRequest(format!("invalid rkey: {rkey}"))),
        None => TID::next_str(None).map_err(|_| ApiError::RuntimeError)?,
    };
    if !session_permits(
        &credentials,
        &did,
        &space_id,
        &SpaceRequest::Write {
            action: SpaceAction::Create,
            collection: collection.clone(),
        },
    ) {
        return Err(ApiError::AuthRequiredError(
            "session does not cover this space".to_string(),
        ));
    }
    let commit = apply_space_writes(
        actor_store,
        blobstore_factory,
        server_config,
        &did,
        &space_id,
        vec![SpaceWrite::Create {
            collection: collection.clone(),
            rkey: rkey.clone(),
            value: record,
        }],
    )
    .await?;
    let cid = commit.results[0].cid.clone().expect("create has a cid");
    Ok(Json(CreateRecordOutput {
        uri: space_id.record_uri(&did, &collection, &rkey),
        cid,
        commit: Some(commit_meta(&commit)),
    }))
}
