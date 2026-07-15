use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::host::local_space_def;
use crate::apis::com::atproto::space::{deliver_notifications, parse_space_uri, space_error};
use crate::apis::ApiError;
use crate::auth_verifier::bearer_token_from_req;
use crate::space_auth::{verify_space_service_token, NOTIFY_WRITE_LXM};
use crate::SharedIdResolver;
use rocket::request::{FromRequest, Outcome, Request};
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::NotifyWriteInput;

pub struct BearerToken(pub String);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for BearerToken {
    type Error = ApiError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match bearer_token_from_req(req) {
            Ok(Some(token)) => Outcome::Success(BearerToken(token)),
            _ => {
                let error = ApiError::AuthRequiredError("service auth required".to_string());
                req.local_cache(|| Some(error.clone()));
                Outcome::Error((rocket::http::Status::Unauthorized, error))
            }
        }
    }
}

/// Inbound write notification (space-host role): a member's repo host reports
/// that a repo advanced. The authority updates its writer set and forwards the
/// notification to registered syncers (spec §Write notifications).
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.space.notifyWrite",
    format = "json",
    data = "<body>"
)]
pub async fn space_notify_write(
    body: Json<NotifyWriteInput>,
    token: BearerToken,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    id_resolver: &State<SharedIdResolver>,
) -> Result<(), ApiError> {
    let NotifyWriteInput { space, did, rev } = body.into_inner();
    let space_id = parse_space_uri(&space)?;
    let claims = verify_space_service_token(actor_store, id_resolver, &token.0, NOTIFY_WRITE_LXM)
        .await
        .map_err(|error| {
            tracing::debug!(%error, "notifyWrite auth rejected");
            ApiError::InvalidToken
        })?;
    // The notification must come from the account whose repo advanced.
    if claims.iss != did {
        return Err(ApiError::InvalidToken);
    }
    let (_, space_store, keypair) =
        local_space_def(actor_store, blobstore_factory, &space_id).await?;
    space_store
        .upsert_writer(&space_id.uri(), &did, &rev, None)
        .await
        .map_err(space_error)?;
    let endpoints = space_store
        .host_notify_endpoints(&space_id.uri(), &rsky_common::now())
        .await
        .map_err(space_error)?;
    if !endpoints.is_empty() {
        let authority = space_id.authority.clone();
        let body = serde_json::json!({ "space": space_id.uri(), "did": did, "rev": rev });
        actor_store.background_queue.add(async move {
            deliver_notifications(
                &keypair,
                &authority,
                &authority,
                NOTIFY_WRITE_LXM,
                &endpoints,
                &body,
            )
            .await;
            Ok(())
        });
    }
    Ok(())
}
