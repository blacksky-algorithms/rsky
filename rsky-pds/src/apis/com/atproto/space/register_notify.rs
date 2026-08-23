use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::space::Subscriber;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::host::local_space_def;
use crate::apis::com::atproto::space::{
    format_expiry, notify_expiry, parse_space_uri, space_error,
};
use crate::apis::ApiError;
use crate::config::ServerConfig;
use crate::space_auth::SpaceCredentialAuth;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::{RegisterNotifyInput, RegisterNotifyOutput};

/// Resolve a `did:...#fragment` service identifier to its endpoint.
///
/// The fragment defaults to `#atproto_space_syncer`, the entry a subscriber
/// publishes for this purpose.
pub async fn resolve_subscriber(plc_url: &str, service: &str) -> Result<String, ApiError> {
    use rsky_identity::did::did_resolver::DidResolver;
    use rsky_identity::types::{DidResolverOpts, MemoryCache};
    use std::sync::Arc;

    let (did, fragment) = match service.split_once('#') {
        Some((did, fragment)) => (did, fragment),
        None => (service, "atproto_space_syncer"),
    };
    let resolver = DidResolver::new(DidResolverOpts {
        timeout: None,
        plc_url: Some(plc_url.to_string()),
        did_cache: Arc::new(MemoryCache::new(None, None)),
    });
    let doc = resolver
        .ensure_resolve(&did.to_string(), None)
        .await
        .map_err(|error| {
            ApiError::InvalidRequest(format!("could not resolve service {service}: {error}"))
        })?;
    doc.service
        .as_deref()
        .unwrap_or_default()
        .iter()
        .find(|entry| {
            entry.id.rsplit_once('#').map(|(_, f)| f) == Some(fragment) || entry.id == fragment
        })
        .map(|entry| entry.service_endpoint.clone())
        .ok_or_else(|| {
            ApiError::InvalidRequest(format!(
                "no {fragment} service in the DID document for {did}"
            ))
        })
}

/// Register a subscriber for write notifications. With a `repo`, subscribes to
/// that repo on this repo host; without one, subscribes to the whole space on
/// this space host (spec §Write notifications).
///
/// A subscriber names itself with `service` (proposals#100), which supplies
/// both where to deliver and who each delivery is addressed to. `endpoint` is
/// the pre-amendment shape and is used only when no `service` is given, in
/// which case deliveries fall back to being addressed to the space authority.
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.space.registerNotify",
    format = "json",
    data = "<body>"
)]
pub async fn space_register_notify(
    body: Json<RegisterNotifyInput>,
    auth: SpaceCredentialAuth,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    server_config: &State<ServerConfig>,
) -> Result<Json<RegisterNotifyOutput>, ApiError> {
    let RegisterNotifyInput {
        space,
        service,
        endpoint,
        repo,
    } = body.into_inner();
    let space_id = parse_space_uri(&space)?;
    if auth.space_uri != space_id.uri() {
        return Err(ApiError::InvalidToken);
    }
    let subscriber = match (&service, &endpoint) {
        (Some(service), _) => Subscriber {
            endpoint: resolve_subscriber(&server_config.identity.plc_url, service).await?,
            service: Some(service.clone()),
        },
        (None, Some(endpoint)) => {
            if !(endpoint.starts_with("https://") || endpoint.starts_with("http://")) {
                return Err(ApiError::InvalidRequest(format!(
                    "invalid endpoint: {endpoint}"
                )));
            }
            Subscriber {
                endpoint: endpoint.clone(),
                service: None,
            }
        }
        (None, None) => {
            return Err(ApiError::InvalidRequest(
                "registerNotify requires a service or an endpoint".to_string(),
            ))
        }
    };
    let expiry = notify_expiry();
    let expires_at = format_expiry(&expiry);
    match repo {
        Some(repo_did) => {
            let reader = actor_store
                .read(
                    repo_did.clone(),
                    blobstore_factory.blobstore(repo_did.clone()),
                )
                .await
                .map_err(|error| {
                    ApiError::BadRequest("RepoNotFound".to_string(), error.to_string())
                })?;
            reader
                .space
                .live_repo_state(&space_id.uri())
                .await
                .map_err(space_error)?;
            reader
                .space
                .register_repo_notify(&space_id.uri(), &subscriber, &expires_at)
                .await
                .map_err(space_error)?;
        }
        None => {
            let (_, space_store, _) =
                local_space_def(actor_store, blobstore_factory, &space_id).await?;
            space_store
                .register_host_notify(&space_id.uri(), &subscriber, &expires_at)
                .await
                .map_err(space_error)?;
        }
    }
    Ok(Json(RegisterNotifyOutput { expires_at: expiry }))
}
