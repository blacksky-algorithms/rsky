use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::space::{blob_cids_in_record, ImportedRecord};
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::{
    parse_space_uri, require_repo_matches_subject, space_error,
};
use crate::apis::ApiError;
use crate::auth_verifier::AccessSpace;
use crate::space_auth::session_permits;
use crate::space_scope::{SpaceAction, SpaceRequest};
use rocket::data::Data;
use rocket::tokio::io::AsyncReadExt;
use rocket::State;
use rsky_space::car::{cid_for_block, RepoCarValidator};
use rsky_space::lthash::{element, LtHash};
use rsky_space::space_id::SpaceId;
use rsky_space::star::{decode_star, Drisl, STAR_MAGIC};

const DAG_CBOR: u64 = 0x71;
const MAX_ARCHIVE: usize = 64 * 1024 * 1024;

/// `com.atproto.space.importRepo`: import the authenticated account's repo
/// in a space from an archive during account migration, replacing whatever
/// this host holds (spec §Repo serialization). Both the spec's STAR archive
/// and the CAR stream `getRepo` serves are accepted.
#[tracing::instrument(skip_all)]
#[rocket::post("/xrpc/com.atproto.space.importRepo?<space>", data = "<body>")]
pub async fn space_import_repo(
    space: Option<String>,
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

    let (space_uri, repo, commit_data, records): (
        String,
        String,
        Option<Vec<u8>>,
        Vec<ImportedRecord>,
    ) = if bytes.len() >= 4 && bytes[..4] == STAR_MAGIC {
        let archive = decode_star(&bytes)
            .map_err(|error| ApiError::InvalidRequest(format!("bad repo archive: {error}")))?;
        let meta = &archive.metadata;
        let (space_uri, repo) = (
            meta.get("space")
                .and_then(Drisl::as_str)
                .unwrap_or_default()
                .to_string(),
            meta.get("repo")
                .and_then(Drisl::as_str)
                .unwrap_or_default()
                .to_string(),
        );
        let commit_data = match meta.get("data") {
            Some(Drisl::Bytes(b)) => Some(b.clone()),
            _ => None,
        };
        let mut records = Vec::with_capacity(archive.pairs.len());
        for (path, value) in &archive.pairs {
            let (collection, rkey) = path
                .split_once('/')
                .ok_or_else(|| ApiError::InvalidRequest(format!("bad record path: {path}")))?;
            let cid = cid_for_block(DAG_CBOR, value).to_string();
            let mut pos = 0;
            let decoded = Drisl::decode(value, &mut pos)
                .map_err(|error| ApiError::InvalidRequest(format!("bad record bytes: {error}")))?;
            records.push(ImportedRecord {
                collection: collection.to_string(),
                rkey: rkey.to_string(),
                cid,
                bytes: value.clone(),
                blob_cids: blob_cids_in_record(&decoded.to_json()),
            });
        }
        (space_uri, repo, commit_data, records)
    } else {
        let validator = RepoCarValidator::new(&bytes[..])
            .await
            .map_err(|error| ApiError::InvalidRequest(format!("bad repo archive: {error}")))?;
        let commit = validator.commit().clone();
        let commit_hash = commit.hash.to_vec();
        let raw = validator
            .into_records(&commit_hash)
            .await
            .map_err(|error| ApiError::InvalidRequest(format!("bad repo archive: {error}")))?;
        let mut records = Vec::with_capacity(raw.len());
        for (path, cid, value) in raw {
            let (collection, rkey) = path
                .split_once('/')
                .ok_or_else(|| ApiError::InvalidRequest(format!("bad record path: {path}")))?;
            let value_json: serde_json::Value =
                rsky_common::cbor_to_struct(value.clone()).map_err(|_| ApiError::RuntimeError)?;
            records.push(ImportedRecord {
                collection: collection.to_string(),
                rkey: rkey.to_string(),
                cid: cid.to_string(),
                bytes: value,
                blob_cids: blob_cids_in_record(&value_json),
            });
        }
        // The CAR commit does not name its space or repo; the space
        // comes from the query param and the repo is the authenticated
        // account's own.
        (
            space.clone().unwrap_or_default(),
            did.clone(),
            Some(commit_hash),
            records,
        )
    };

    if space_uri.is_empty() {
        return Err(ApiError::InvalidRequest(
            "a space is required when importing a CAR archive".to_string(),
        ));
    }
    let space_id: SpaceId = parse_space_uri(&space_uri)?;
    require_repo_matches_subject(&repo, &did)?;
    // If the account is the space's authority, the space itself must exist
    // (importSpaceMeta runs first, spec §Account migration).
    let reader = actor_store
        .read(did.clone(), blobstore_factory.blobstore(did.clone()))
        .await
        .map_err(|error| ApiError::BadRequest("RepoNotFound".to_string(), error.to_string()))?;
    if space_id.authority == did
        && reader
            .space
            .get_space_def(&space_id.uri())
            .await
            .map_err(space_error)?
            .is_none()
    {
        return Err(ApiError::BadRequest(
            "SpaceNotFound".to_string(),
            "import the space's meta archive first".to_string(),
        ));
    }
    // A create grant is required for every collection in the archive.
    for collection in records
        .iter()
        .map(|r| r.collection.clone())
        .collect::<std::collections::BTreeSet<_>>()
    {
        if !session_permits(
            &credentials,
            &did,
            &space_id,
            &SpaceRequest::Write {
                action: SpaceAction::Create,
                collection,
            },
        ) {
            return Err(ApiError::AuthRequiredError(
                "session does not cover importing this repo".to_string(),
            ));
        }
    }
    // The archive's commit digest is recomputed from its records: a mismatch
    // means the archive is corrupt or truncated.
    if let Some(want) = commit_data {
        let mut lthash = LtHash::new();
        for record in &records {
            lthash.add(&element(&record.collection, &record.rkey, &record.cid));
        }
        if lthash.hash().as_slice() != want.as_slice() {
            return Err(ApiError::InvalidRequest(
                "repo archive digest does not match its commit".to_string(),
            ));
        }
    }
    reader
        .space
        .replace_repo(&space_id, records)
        .await
        .map_err(space_error)?;
    Ok(())
}
