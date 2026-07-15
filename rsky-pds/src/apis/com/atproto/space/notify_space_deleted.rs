use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::notify_write::BearerToken;
use crate::apis::com::atproto::space::{internal_error, parse_space_uri};
use crate::apis::ApiError;
use crate::space_auth::{verify_space_service_token, NOTIFY_SPACE_DELETED_LXM};
use crate::SharedIdResolver;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::NotifySpaceDeletedInput;

/// Inbound space-deletion notification (repo-host role): flag every local
/// repo in the space as deleted. Never erases record rows: the data is the
/// user's own (spec §Space deletion).
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.space.notifySpaceDeleted",
    format = "json",
    data = "<body>"
)]
pub async fn space_notify_space_deleted(
    body: Json<NotifySpaceDeletedInput>,
    token: BearerToken,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    id_resolver: &State<SharedIdResolver>,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    let NotifySpaceDeletedInput { space } = body.into_inner();
    let space_id = parse_space_uri(&space)?;
    let claims =
        verify_space_service_token(actor_store, id_resolver, &token.0, NOTIFY_SPACE_DELETED_LXM)
            .await
            .map_err(|error| {
                tracing::debug!(%error, "notifySpaceDeleted auth rejected");
                ApiError::InvalidToken
            })?;
    // Only the space authority may announce the space's deletion.
    let iss_did = claims.iss.split('#').next().unwrap_or(&claims.iss);
    if iss_did != space_id.authority {
        return Err(ApiError::InvalidToken);
    }
    let dids: Vec<String> = account_manager
        .db
        .run(|conn| {
            let mut stmt = conn.prepare("SELECT did FROM actor ORDER BY did")?;
            let rows = stmt
                .query_map([], |row| row.get(0))?
                .collect::<Result<Vec<String>, rusqlite::Error>>()?;
            Ok(rows)
        })
        .await
        .map_err(internal_error("account listing failed"))?;
    for did in dids {
        let Ok(reader) = actor_store
            .read(did.clone(), blobstore_factory.blobstore(did.clone()))
            .await
        else {
            continue;
        };
        match reader.space.flag_repo_deleted(&space_id.uri()).await {
            Ok(true) => tracing::info!(%did, space = %space_id.uri(), "flagged repo deleted"),
            Ok(false) => {}
            Err(error) => tracing::warn!(%error, %did, "failed to flag repo deleted"),
        }
    }
    Ok(())
}
