use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::space::{SpaceDefRow, SpacePolicy};
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::host::parse_policy;
use crate::apis::com::atproto::space::{
    parse_space_uri, require_manage, space_def_error,
};
use crate::apis::ApiError;
use crate::auth_verifier::AccessSpace;
use crate::space_scope::ManageOp;
use rocket::data::Data;
use rocket::tokio::io::AsyncReadExt;
use rocket::State;
use rsky_space::star::{decode_star, Drisl};

const MAX_ARCHIVE: usize = 16 * 1024 * 1024;

/// `com.atproto.space.importSpaceMeta`: restore a space's host-side state
/// from an `atproto-space-meta` STAR archive during account migration
/// (spec §Space meta archive).
#[tracing::instrument(skip_all)]
#[rocket::post("/xrpc/com.atproto.space.importSpaceMeta", data = "<body>")]
pub async fn space_import_space_meta(
    body: Data<'_>,
    auth: AccessSpace,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
) -> Result<(), ApiError> {
    let credentials = auth.access.credentials.expect("credentials populated");
    let did = credentials.did.clone().expect("did populated");
    let mut bytes = Vec::new();
    body.open((MAX_ARCHIVE as u64).into())
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| ApiError::RuntimeError)?;
    let archive = decode_star(&bytes)
        .map_err(|error| ApiError::InvalidRequest(format!("bad space meta archive: {error}")))?;
    let Some(space) = archive.metadata.get("space").and_then(Drisl::as_str) else {
        return Err(ApiError::InvalidRequest(
            "space meta archive without a space".to_string(),
        ));
    };
    let space_id = parse_space_uri(space)?;
    if space_id.authority != did {
        return Err(ApiError::BadRequest(
            "NotSpaceOwner".to_string(),
            "only the space authority may import a space".to_string(),
        ));
    }
    require_manage(&credentials, &did, &space_id, ManageOp::Create)?;
    require_manage(&credentials, &did, &space_id, ManageOp::Update)?;

    let reader = actor_store
        .read(did.clone(), blobstore_factory.blobstore(did.clone()))
        .await
        .map_err(|error| ApiError::BadRequest("RepoNotFound".to_string(), error.to_string()))?;

    // Decode the archive body first: every key must be one this host
    // implements before the stored state is touched.
    let mut policy: Option<SpacePolicy> = None;
    let mut members: Vec<(String, bool, bool)> = Vec::new();
    let mut writers: Vec<(String, String, Option<String>)> = Vec::new();
    for (key, value) in &archive.pairs {
        let mut pos = 0;
        let decoded = Drisl::decode(value, &mut pos)
            .map_err(|error| ApiError::InvalidRequest(format!("bad archive value: {error}")))?;
        let value_json = decoded.to_json();
        if key == "policy" {
            policy = Some(parse_policy(value_json)?);
        } else if let Some(member) = key.strip_prefix("com.atproto.memberspace.member/") {
            if !member.starts_with("did:") {
                return Err(ApiError::InvalidRequest(format!(
                    "malformed member key: {key}"
                )));
            }
            let read = value_json["read"].as_bool();
            let write = value_json["write"].as_bool();
            match (read, write) {
                (Some(read), Some(write)) => {
                    members.push((member.to_string(), read, write));
                }
                _ => {
                    return Err(ApiError::InvalidRequest(format!(
                        "malformed member value: {key}"
                    )))
                }
            }
        } else if let Some(writer) = key.strip_prefix("writer/") {
            if !writer.starts_with("did:") {
                return Err(ApiError::InvalidRequest(format!(
                    "malformed writer key: {key}"
                )));
            }
            let Some(rev) = value_json["rev"].as_str() else {
                return Err(ApiError::InvalidRequest(format!(
                    "malformed writer value: {key}"
                )));
            };
            let hash = match decoded.get("data") {
                Some(Drisl::Bytes(bytes)) => Some(hex::encode(bytes)),
                Some(Drisl::Str(hash)) => Some(hash.clone()),
                _ => None,
            };
            writers.push((writer.to_string(), rev.to_string(), hash));
        } else {
            return Err(ApiError::BadRequest(
                "UnsupportedPolicy".to_string(),
                format!("unrecognized policy state key: {key}"),
            ));
        }
    }
    let Some(policy) = policy else {
        return Err(ApiError::InvalidRequest(
            "space meta archive without a policy".to_string(),
        ));
    };

    if reader
        .space
        .get_space_def(&space_id.uri())
        .await
        .map_err(space_def_error)?
        .is_some()
    {
        reader
            .space
            .update_space_def(SpaceDefRow {
                space_uri: space_id.uri(),
                space_type: space_id.space_type.clone(),
                skey: space_id.skey.clone(),
                policy,
                deleted: false,
            })
            .await
            .map_err(space_def_error)?;
    } else {
        reader
            .space
            .create_space_def(SpaceDefRow {
                space_uri: space_id.uri(),
                space_type: space_id.space_type.clone(),
                skey: space_id.skey.clone(),
                policy,
                deleted: false,
            })
            .await
            .map_err(space_def_error)?;
    }
    reader
        .space
        .clear_members(&space_id.uri())
        .await
        .map_err(space_def_error)?;
    for (member, read, write) in members {
        reader
            .space
            .put_member(&space_id.uri(), &member, read, write)
            .await
            .map_err(space_def_error)?;
    }
    reader
        .space
        .clear_writers(&space_id.uri())
        .await
        .map_err(space_def_error)?;
    for (writer, rev, hash) in writers {
        reader
            .space
            .upsert_writer(&space_id.uri(), &writer, &rev, hash)
            .await
            .map_err(space_def_error)?;
    }
    Ok(())
}
