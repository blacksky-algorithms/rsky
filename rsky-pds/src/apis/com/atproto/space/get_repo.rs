use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::space::commit::sign_commit;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::{
    internal_error, open_local_repo, parse_space_uri, space_error,
};
use crate::apis::ApiError;
use crate::space_auth::{authorize_space_read, SpaceReadAuth};
use crate::space_scope::SpaceRequest;
use lexicon_cid::Cid;
use rocket::http::Header;
use rocket::{Responder, State};
use rsky_space::car::repo_car_bytes;
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;

#[derive(Responder)]
#[response(status = 200)]
pub struct CarResponder(Vec<u8>, Header<'static>);

/// Serialize a whole permissioned repo as a two-root CAR (signed commit +
/// index), records in lexicographic path order (spec §Repo serialization).
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.space.getRepo?<space>&<did>")]
pub async fn space_get_repo(
    space: String,
    did: String,
    auth: SpaceReadAuth,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    account_manager: AccountManager,
) -> Result<CarResponder, ApiError> {
    let space_id = parse_space_uri(&space)?;
    authorize_space_read(
        &auth,
        &space_id,
        &did,
        &SpaceRequest::ReadSelf { collection: None },
    )?;
    let reader = open_local_repo(
        actor_store,
        blobstore_factory,
        &account_manager,
        &did,
        false,
    )
    .await?;
    let state = reader
        .space
        .live_repo_state(&space_id.uri())
        .await
        .map_err(space_error)?;
    let rows = reader
        .space
        .all_records(&space_id.uri())
        .await
        .map_err(space_error)?;
    let mut entries: BTreeMap<String, Cid> = BTreeMap::new();
    let mut blocks: HashMap<Cid, Vec<u8>> = HashMap::new();
    for row in rows {
        let cid = Cid::from_str(&row.cid).map_err(internal_error("stored cid failed to parse"))?;
        entries.insert(format!("{}/{}", row.collection, row.rkey), cid);
        blocks.insert(cid, row.value);
    }
    let keypair = reader
        .keypair()
        .await
        .map_err(internal_error("missing actor keypair"))?;
    let commit = sign_commit(&keypair, &space_id.uri(), &did, &state.rev, &state.hash())
        .map_err(internal_error("commit signing failed"))?;
    let car = repo_car_bytes(&commit, &entries, move |cid| blocks.get(cid).cloned())
        .await
        .map_err(internal_error("car serialization failed"))?;
    Ok(CarResponder(
        car,
        Header::new("content-type", "application/vnd.ipld.car"),
    ))
}
