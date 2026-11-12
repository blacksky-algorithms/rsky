use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::space::decode_record;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::kem::ActorKemKeys;
use crate::apis::com::atproto::space::{
    internal_error, open_local_repo, parse_space_uri, space_error,
};
use crate::apis::ApiError;
use crate::space_auth::{authorize_space_read, SpaceReadAuth};
use crate::space_mac::{
    build_record_uri, compute_response_mac, MacResponse, MacType, WantMacHeader,
};
use crate::space_scope::SpaceRequest;
use lexicon_cid::Cid;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::GetRecordOutput;
use std::str::FromStr;

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.space.getRecord?<space>&<repo>&<collection>&<rkey>&<cid>")]
#[allow(clippy::too_many_arguments)]
pub async fn space_get_record(
    space: String,
    repo: String,
    collection: String,
    rkey: String,
    cid: Option<String>,
    auth: SpaceReadAuth,
    want_mac: WantMacHeader,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    account_manager: AccountManager,
) -> Result<MacResponse<Json<GetRecordOutput>>, ApiError> {
    let space_id = parse_space_uri(&space)?;
    authorize_space_read(
        &auth,
        &space_id,
        &repo,
        &SpaceRequest::ReadSelf {
            collection: Some(collection.clone()),
        },
    )?;
    let reader = open_local_repo(
        actor_store,
        blobstore_factory,
        &account_manager,
        &repo,
        false,
        &auth,
    )
    .await?;
    reader
        .space
        .live_repo_state(&space_id.uri())
        .await
        .map_err(space_error)?;
    let record = reader
        .space
        .get_record(&space_id.uri(), &collection, &rkey)
        .await
        .map_err(space_error)?
        .ok_or(ApiError::RecordNotFound)?;
    if let Some(ref cid_param) = cid {
        if record.cid != *cid_param {
            return Err(ApiError::RecordNotFound);
        }
    }
    let value =
        decode_record(&record.value).map_err(internal_error("stored record failed to decode"))?;

    // Build record URI for MAC: {space}/{repo}/{collection}/{rkey}
    let record_uri = build_record_uri(&space, &repo, &collection, &rkey);

    // Compute MAC if requested
    let mac = if let Some(ref want) = want_mac.0 {
        // Validate MAC type matches method
        if want.mac_type != MacType::Record {
            return Err(ApiError::BadRequest(
                "MacTypeMismatch".to_string(),
                format!(
                    "getRecord requires type=\"atproto-space-record\", got {:?}",
                    want.mac_type
                ),
            ));
        }

        // Get CID bytes for MAC computation
        let cid_obj =
            Cid::from_str(&record.cid).map_err(internal_error("stored cid failed to parse"))?;
        let cid_bytes = cid_obj.to_bytes();

        // MAC under the author's `#atproto_kem` key; an account that has none
        // yet answers without a MAC.
        let kem_provider = ActorKemKeys(actor_store);
        compute_response_mac(
            &kem_provider,
            want,
            &repo,
            &space,
            &repo,
            None,
            None,
            Some(&record_uri),
            Some(&cid_bytes),
        )
        .await
        .ok() // Gracefully handle missing KEM key
    } else {
        None
    };

    Ok(MacResponse::new(
        Json(GetRecordOutput {
            uri: space_id.record_uri(&repo, &collection, &rkey),
            cid: Some(record.cid),
            value,
        }),
        mac,
        &want_mac,
    ))
}
