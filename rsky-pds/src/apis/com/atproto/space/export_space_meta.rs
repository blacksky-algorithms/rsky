use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::space::SpacePolicy;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::{
    parse_space_uri, require_read_self, space_def_error,
};
use crate::apis::ApiError;
use crate::auth_verifier::AccessSpace;
use rocket::http::Header;
use rocket::{Responder, State};
use rsky_space::star::{encode_star, Drisl, PROFILE_SPACE_META};
use serde_json::json;

#[derive(Responder)]
#[response(status = 200)]
pub struct OctetResponder(Vec<u8>, Header<'static>);

/// `com.atproto.space.exportSpaceMeta`: the space's host-side state as an
/// `atproto-space-meta` STAR archive — the policy, the policy's state (the
/// memberspace member list), and the writer set (spec §Space meta archive).
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.space.exportSpaceMeta?<space>")]
pub async fn space_export_space_meta(
    space: String,
    auth: AccessSpace,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
) -> Result<OctetResponder, ApiError> {
    let space_id = parse_space_uri(&space)?;
    let credentials = auth.access.credentials.expect("credentials populated");
    let did = credentials.did.clone().expect("did populated");
    if space_id.authority != did {
        return Err(ApiError::BadRequest(
            "NotSpaceOwner".to_string(),
            "only the space authority may export a space".to_string(),
        ));
    }
    require_read_self(&credentials, &did, &space_id)?;
    let reader = actor_store
        .read(did.clone(), blobstore_factory.blobstore(did.clone()))
        .await
        .map_err(|error| ApiError::BadRequest("RepoNotFound".to_string(), error.to_string()))?;
    let def = reader
        .space
        .live_space_def(&space_id.uri())
        .await
        .map_err(space_def_error)?;

    let mut pairs: Vec<(String, Vec<u8>)> = vec![(
        "policy".to_string(),
        Drisl::from_json(&serde_json::to_value(&def.policy).expect("policy serializes"))
            .map_err(|_| ApiError::RuntimeError)?
            .encode_to_vec(),
    )];
    if let SpacePolicy::Memberspace(_) = def.policy {
        let mut cursor = None;
        loop {
            let page = reader
                .space
                .list_members(&space_id.uri(), 500, cursor.clone())
                .await
                .map_err(space_def_error)?;
            let full_page = page.len() == 500;
            for row in &page {
                pairs.push((
                    format!("com.atproto.memberspace.member/{}", row.did),
                    Drisl::from_json(&json!({
                        "$type": "com.atproto.memberspace.member",
                        "read": row.read,
                        "write": row.write,
                    }))
                    .map_err(|_| ApiError::RuntimeError)?
                    .encode_to_vec(),
                ));
            }
            cursor = page.last().map(|row| row.did.clone());
            if !full_page {
                break;
            }
        }
    }
    let writers = reader
        .space
        .list_writers(&space_id.uri(), usize::MAX >> 1, None)
        .await
        .map_err(space_def_error)?;
    for writer in writers {
        let data = writer
            .hash
            .as_ref()
            .and_then(|h| hex::decode(h).ok())
            .map(Drisl::Bytes)
            .unwrap_or_else(|| writer.hash.as_deref().map(|h| Drisl::Str(h.to_string())).unwrap_or(Drisl::Null));
        pairs.push((
            format!("writer/{}", writer.did),
            Drisl::Map(vec![
                ("rev".to_string(), Drisl::Str(writer.rev)),
                ("data".to_string(), data),
            ])
            .encode_to_vec(),
        ));
    }
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let metadata = Drisl::from_json(&json!({
        "$type": PROFILE_SPACE_META,
        "ver": 1,
        "space": space_id.uri(),
    }))
    .map_err(|_| ApiError::RuntimeError)?;
    Ok(OctetResponder(
        encode_star(&metadata, &pairs),
        Header::new("content-type", "application/octet-stream"),
    ))
}
