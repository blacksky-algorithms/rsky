use crate::actor_store::aws::s3::S3BlobStore;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::AccessFullImport;
use crate::db::DbConn;
use crate::repo::prepare::{
    prepare_create, prepare_delete, prepare_update, PrepareCreateOpts, PrepareDeleteOpts,
    PrepareUpdateOpts,
};
use aws_config::SdkConfig;
use futures::{stream, StreamExt};
use lexicon_cid::Cid;
use reqwest::header;
use rocket::data::{FromData, Outcome, ToByteUnit};
use rocket::http::Status;
use rocket::{Data, Request, State};
use rsky_common::env::env_int;
use rsky_repo::block_map::BlockMap;
use rsky_repo::car::{read_stream_car_with_root, CarWithRoot};
use rsky_repo::parse::get_and_parse_record;
use rsky_repo::repo::Repo;
use rsky_repo::sync::consumer::{verify_diff, VerifyRepoInput};
use rsky_repo::types::{PreparedWrite, RecordWriteDescript, VerifiedDiff};

struct ImportRepoInput {
    car_with_root: CarWithRoot,
}

#[rocket::async_trait]
impl<'r> FromData<'r> for ImportRepoInput {
    type Error = ApiError;

    #[tracing::instrument(skip_all)]
    async fn from_data(req: &'r Request<'_>, data: Data<'r>) -> Outcome<'r, Self, Self::Error> {
        let max_import_size = env_int("IMPORT_REPO_LIMIT").unwrap_or(100).megabytes();
        match req.headers().get_one(header::CONTENT_LENGTH.as_ref()) {
            None => {
                let error = ApiError::InvalidRequest("Missing content-length header".to_string());
                req.local_cache(|| Some(error.clone()));
                Outcome::Error((Status::BadRequest, error))
            }
            Some(res) => match res.parse::<usize>() {
                Ok(content_length) => {
                    if content_length.bytes() > max_import_size {
                        let error = ApiError::InvalidRequest(format!(
                            "Content-Length is greater than maximum of {max_import_size}"
                        ));
                        req.local_cache(|| Some(error.clone()));
                        return Outcome::Error((Status::BadRequest, error));
                    }

                    let import_datastream = data.open(content_length.bytes());
                    match read_stream_car_with_root(import_datastream).await {
                        Ok(car_with_root) => Outcome::Success(ImportRepoInput { car_with_root }),
                        Err(error) => {
                            let error = ApiError::InvalidRequest(error.to_string());
                            req.local_cache(|| Some(error.clone()));
                            Outcome::Error((Status::BadRequest, error))
                        }
                    }
                }
                Err(_error) => {
                    tracing::error!("{}", format!("Error parsing content-length\n{_error}"));
                    let error =
                        ApiError::InvalidRequest("Error parsing content-length".to_string());
                    req.local_cache(|| Some(error.clone()));
                    Outcome::Error((Status::BadRequest, error))
                }
            },
        }
    }
}

#[tracing::instrument(skip_all)]
#[rocket::post("/xrpc/com.atproto.repo.importRepo", data = "<import_repo_input>")]
pub async fn import_repo(
    auth: AccessFullImport,
    import_repo_input: ImportRepoInput,
    s3_config: &State<SdkConfig>,
    db: DbConn,
) -> Result<(), ApiError> {
    let requester = auth.access.credentials.unwrap().did.unwrap();
    let mut actor_store = ActorStore::new(
        requester.clone(),
        S3BlobStore::new(requester.clone(), s3_config),
        db,
    );

    // Get current repo if it exists
    let curr_root: Option<Cid> = actor_store.get_repo_root().await;
    let curr_repo: Option<Repo> = match curr_root {
        None => None,
        Some(_root) => Some(Repo::load(actor_store.storage.clone(), curr_root).await?),
    };

    // Process imported car
    let car_with_root = import_repo_input.car_with_root;

    // Get verified difference from current repo and imported repo
    let mut imported_blocks: BlockMap = car_with_root.blocks;
    let imported_root: Cid = car_with_root.root;
    let opts = VerifyRepoInput {
        ensure_leaves: Some(false),
    };

    let diff: VerifiedDiff = match verify_diff(
        curr_repo,
        &mut imported_blocks,
        imported_root,
        None,
        None,
        Some(opts),
    )
    .await
    {
        Ok(res) => res,
        Err(error) => {
            tracing::error!("{:?}", error);
            return Err(ApiError::RuntimeError);
        }
    };

    let commit_data = diff.commit;
    let prepared_writes: Vec<PreparedWrite> =
        prepare_import_repo_writes(requester, diff.writes, &imported_blocks).await?;
    match actor_store
        .process_import_repo(commit_data, prepared_writes)
        .await
    {
        Ok(_res) => {}
        Err(error) => {
            tracing::error!("Error importing repo\n{error}");
            return Err(ApiError::RuntimeError);
        }
    }

    Ok(())
}

/// Converts list of RecordWriteDescripts into a list of PreparedWrites
async fn prepare_import_repo_writes(
    _did: String,
    writes: Vec<RecordWriteDescript>,
    blocks: &BlockMap,
) -> Result<Vec<PreparedWrite>, ApiError> {
    match stream::iter(writes)
        .then(|write| {
            let did = _did.clone();
            async move {
                Ok::<PreparedWrite, anyhow::Error>(match write {
                    RecordWriteDescript::Create(write) => {
                        let parsed_record = get_and_parse_record(blocks, write.cid)?;
                        PreparedWrite::Create(
                            prepare_create(PrepareCreateOpts {
                                did: did.clone(),
                                collection: write.collection,
                                rkey: Some(write.rkey),
                                swap_cid: None,
                                record: parsed_record.record,
                                validate: Some(true),
                            })
                            .await?,
                        )
                    }
                    RecordWriteDescript::Update(write) => {
                        let parsed_record = get_and_parse_record(blocks, write.cid)?;
                        PreparedWrite::Update(
                            prepare_update(PrepareUpdateOpts {
                                did: did.clone(),
                                collection: write.collection,
                                rkey: write.rkey,
                                swap_cid: None,
                                record: parsed_record.record,
                                validate: Some(true),
                            })
                            .await?,
                        )
                    }
                    RecordWriteDescript::Delete(write) => {
                        PreparedWrite::Delete(prepare_delete(PrepareDeleteOpts {
                            did: did.clone(),
                            collection: write.collection,
                            rkey: write.rkey,
                            swap_cid: None,
                        })?)
                    }
                })
            }
        })
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<anyhow::Result<Vec<PreparedWrite>, _>>()
    {
        Ok(res) => Ok(res),
        Err(error) => {
            tracing::error!("Error preparing import repo writes\n{error}");
            Err(ApiError::RuntimeError)
        }
    }
}
