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
use rsky_lexicon::com::atproto::space::{
    ApplyWritesInput, ApplyWritesOutput, ApplyWritesResult, ApplyWritesWrite, CreateResult,
    DeleteResult, UpdateResult,
};

const MAX_WRITES: usize = 200;

/// Apply a batch of creates, updates, and deletes to the caller's repo in one
/// space atomically.
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.space.applyWrites",
    format = "json",
    data = "<body>"
)]
pub async fn space_apply_writes(
    body: Json<ApplyWritesInput>,
    auth: AccessFull,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    server_config: &State<ServerConfig>,
) -> Result<Json<ApplyWritesOutput>, ApiError> {
    let ApplyWritesInput {
        space,
        validate: _,
        writes,
    } = body.into_inner();
    let space_id = parse_space_uri(&space)?;
    let credentials = auth.access.credentials.expect("credentials populated");
    let did = credentials.did.clone().expect("did populated");
    if writes.len() > MAX_WRITES {
        return Err(ApiError::InvalidRequest(format!(
            "too many writes. max: {MAX_WRITES}"
        )));
    }
    let mut prepared = Vec::with_capacity(writes.len());
    for write in writes {
        let (action, collection, rkey, value) = match write {
            ApplyWritesWrite::Create(create) => {
                let rkey = match create.rkey {
                    Some(rkey) => rkey,
                    None => TID::next_str(None).map_err(|_| ApiError::RuntimeError)?,
                };
                (
                    SpaceAction::Create,
                    create.collection,
                    rkey,
                    Some(create.value),
                )
            }
            ApplyWritesWrite::Update(update) => (
                SpaceAction::Update,
                update.collection,
                update.rkey,
                Some(update.value),
            ),
            ApplyWritesWrite::Delete(delete) => {
                (SpaceAction::Delete, delete.collection, delete.rkey, None)
            }
        };
        if !valid_nsid(&collection) {
            return Err(ApiError::InvalidRequest(format!(
                "invalid collection: {collection}"
            )));
        }
        if !valid_key_part(&rkey, 512) {
            return Err(ApiError::InvalidRequest(format!("invalid rkey: {rkey}")));
        }
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
        prepared.push(match (action, value) {
            (SpaceAction::Create, Some(value)) => SpaceWrite::Create {
                collection,
                rkey,
                value,
            },
            (SpaceAction::Update, Some(value)) => SpaceWrite::Update {
                collection,
                rkey,
                value,
                swap_cid: None,
            },
            _ => SpaceWrite::Delete {
                collection,
                rkey,
                swap_cid: None,
            },
        });
    }
    let commit = apply_space_writes(
        actor_store,
        blobstore_factory,
        server_config,
        &did,
        &space_id,
        prepared,
    )
    .await?;
    let results = commit
        .results
        .iter()
        .map(|result| {
            let uri = space_id.record_uri(&did, &result.collection, &result.rkey);
            match (&result.cid, &result.prev) {
                (Some(cid), None) => ApplyWritesResult::Create(CreateResult {
                    uri,
                    cid: cid.clone(),
                }),
                (Some(cid), Some(_)) => ApplyWritesResult::Update(UpdateResult {
                    uri,
                    cid: cid.clone(),
                }),
                _ => ApplyWritesResult::Delete(DeleteResult {}),
            }
        })
        .collect();
    Ok(Json(ApplyWritesOutput {
        commit: Some(commit_meta(&commit)),
        results: Some(results),
    }))
}
