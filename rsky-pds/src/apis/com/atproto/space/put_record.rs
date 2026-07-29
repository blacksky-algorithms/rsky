use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::space::SpaceWrite;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::{
    apply_space_writes, commit_meta, parse_space_uri, space_error, valid_key_part, valid_nsid,
};
use crate::apis::ApiError;
use crate::auth_verifier::AccessFull;
use crate::config::ServerConfig;
use crate::space_auth::session_permits;
use crate::space_scope::{SpaceAction, SpaceRequest};
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::{PutRecordInput, PutRecordOutput};

/// Create or update a record in the caller's permissioned repo.
#[tracing::instrument(skip_all)]
#[rocket::post("/xrpc/com.atproto.space.putRecord", format = "json", data = "<body>")]
pub async fn space_put_record(
    body: Json<PutRecordInput>,
    auth: AccessFull,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    server_config: &State<ServerConfig>,
) -> Result<Json<PutRecordOutput>, ApiError> {
    let PutRecordInput {
        space,
        collection,
        rkey,
        validate: _,
        record,
        swap_record,
    } = body.into_inner();
    let space_id = parse_space_uri(&space)?;
    let credentials = auth.access.credentials.expect("credentials populated");
    let did = credentials.did.clone().expect("did populated");
    if !valid_nsid(&collection) {
        return Err(ApiError::InvalidRequest(format!(
            "invalid collection: {collection}"
        )));
    }
    if !valid_key_part(&rkey, 512) {
        return Err(ApiError::InvalidRequest(format!("invalid rkey: {rkey}")));
    }
    let exists = {
        let reader = actor_store
            .read(did.clone(), blobstore_factory.blobstore(did.clone()))
            .await
            .map_err(|error| ApiError::BadRequest("RepoNotFound".to_string(), error.to_string()))?;
        match reader.space.repo_state(&space_id.uri()).await {
            Ok(Some(state)) if state.deleted => {
                return Err(space_error(
                    crate::actor_store::space::SpaceStoreError::SpaceDeleted(space_id.uri()).into(),
                ))
            }
            Ok(Some(_)) => reader
                .space
                .get_record(&space_id.uri(), &collection, &rkey)
                .await
                .map_err(space_error)?
                .is_some(),
            Ok(None) => false,
            Err(error) => return Err(space_error(error)),
        }
    };
    let action = if exists {
        SpaceAction::Update
    } else {
        SpaceAction::Create
    };
    if !session_permits(
        &credentials,
        &did,
        &space_id,
        &SpaceRequest::Write {
            action,
            collection: collection.clone(),
        },
    ) {
        return Err(ApiError::AuthRequiredError(
            "session does not cover this space".to_string(),
        ));
    }
    let write = if exists {
        SpaceWrite::Update {
            collection: collection.clone(),
            rkey: rkey.clone(),
            value: record,
            swap_cid: swap_record,
        }
    } else {
        SpaceWrite::Create {
            collection: collection.clone(),
            rkey: rkey.clone(),
            value: record,
        }
    };
    let commit = apply_space_writes(
        actor_store,
        blobstore_factory,
        server_config,
        &did,
        &space_id,
        vec![write],
    )
    .await?;
    let cid = commit.results[0].cid.clone().expect("put has a cid");
    Ok(Json(PutRecordOutput {
        uri: space_id.record_uri(&did, &collection, &rkey),
        cid,
        commit: Some(commit_meta(&commit)),
    }))
}
