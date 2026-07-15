//! `com.atproto.space.*` routes: the repo-host, space-host, and PDS method
//! groups of the permissioned-data proposal.

use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::space::commit::{sign_commit, to_lexicon};
use crate::actor_store::space::{
    oplog_window, SpaceCommitResult, SpaceStore, SpaceStoreError, SpaceWrite,
};
use crate::actor_store::{ActorStore, ActorStoreReader};
use crate::apis::com::atproto::repo::assert_repo_availability;
use crate::apis::ApiError;
use crate::config::ServerConfig;
use crate::space_auth::{mint_space_service_token, NOTIFY_SPACE_DELETED_LXM, NOTIFY_WRITE_LXM};
use anyhow::Result;
use rocket::State;
use rsky_lexicon::com::atproto::space::CommitMeta;
use rsky_space::space_id::SpaceId;
use secp256k1::Keypair;

pub mod apply_writes;
pub mod create_record;
pub mod delete_record;
pub mod get_blob;
pub mod get_delegation_token;
pub mod get_latest_commit;
pub mod get_record;
pub mod get_repo;
pub mod get_space;
pub mod get_space_credential;
pub mod host;
pub mod list_records;
pub mod list_repo_ops;
pub mod list_repos;
pub mod list_spaces;
pub mod notify_space_deleted;
pub mod notify_write;
pub mod put_record;
pub mod register_notify;

/// How long a notify registration (explicit or auto) stays live.
pub const NOTIFY_REGISTRATION_TTL_SECS: i64 = 7 * 24 * 3600;

pub fn notify_expiry() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc::now() + chrono::Duration::seconds(NOTIFY_REGISTRATION_TTL_SECS)
}

pub fn format_expiry(expiry: &chrono::DateTime<chrono::Utc>) -> String {
    expiry.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

pub fn parse_space_uri(space: &str) -> Result<SpaceId, ApiError> {
    SpaceId::parse(space)
        .map_err(|_| ApiError::InvalidRequest(format!("invalid space uri: {space}")))
}

pub fn space_error(error: anyhow::Error) -> ApiError {
    match error.downcast_ref::<SpaceStoreError>() {
        Some(SpaceStoreError::SpaceNotFound(_)) => {
            ApiError::BadRequest("SpaceNotFound".to_string(), error.to_string())
        }
        Some(SpaceStoreError::SpaceDeleted(_)) => {
            ApiError::BadRequest("SpaceDeleted".to_string(), error.to_string())
        }
        Some(SpaceStoreError::RecordExists(_)) => {
            ApiError::BadRequest("RecordExists".to_string(), error.to_string())
        }
        Some(SpaceStoreError::RecordNotFound(_)) => ApiError::RecordNotFound,
        Some(SpaceStoreError::HistoryUnavailable) => {
            ApiError::BadRequest("HistoryUnavailable".to_string(), error.to_string())
        }
        Some(SpaceStoreError::InvalidSwap(_)) => {
            ApiError::BadRequest("InvalidSwap".to_string(), error.to_string())
        }
        None => {
            tracing::error!("space route error: {error}");
            ApiError::RuntimeError
        }
    }
}

/// Open a read handle on a locally hosted, available repo.
pub async fn open_local_repo(
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    account_manager: &AccountManager,
    did: &str,
    is_user_or_admin: bool,
) -> Result<ActorStoreReader, ApiError> {
    assert_repo_availability(&did.to_string(), is_user_or_admin, account_manager)
        .await
        .map_err(|error| ApiError::BadRequest("RepoNotFound".to_string(), error.to_string()))?;
    actor_store
        .read(
            did.to_string(),
            blobstore_factory.blobstore(did.to_string()),
        )
        .await
        .map_err(|error| ApiError::BadRequest("RepoNotFound".to_string(), error.to_string()))
}

/// Sign the current commit for a repo at serve time.
pub async fn serve_commit(
    reader: &ActorStoreReader,
    space_uri: &str,
) -> Result<rsky_lexicon::com::atproto::space::SignedCommit, ApiError> {
    let state = reader
        .space
        .live_repo_state(space_uri)
        .await
        .map_err(space_error)?;
    let keypair = reader.keypair().await.map_err(|error| {
        tracing::error!("missing actor keypair: {error}");
        ApiError::RuntimeError
    })?;
    let commit = sign_commit(&keypair, space_uri, &reader.did, &state.rev, &state.hash()).map_err(
        |error| {
            tracing::error!("commit signing failed: {error}");
            ApiError::RuntimeError
        },
    )?;
    Ok(to_lexicon(&commit))
}

pub fn commit_meta(commit: &SpaceCommitResult) -> CommitMeta {
    CommitMeta {
        rev: commit.rev.clone(),
        hash: hex::encode(commit.hash),
    }
}

/// Apply writes to the caller's repo in a space, then fan out best-effort
/// write notifications (spec §Write notifications).
pub async fn apply_space_writes(
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    server_config: &State<ServerConfig>,
    did: &str,
    space: &SpaceId,
    writes: Vec<SpaceWrite>,
) -> Result<SpaceCommitResult, ApiError> {
    let transactor = actor_store
        .transact(
            did.to_string(),
            blobstore_factory.blobstore(did.to_string()),
        )
        .await
        .map_err(|error| ApiError::BadRequest("RepoNotFound".to_string(), error.to_string()))?;
    let commit = transactor
        .space
        .apply_writes(space, writes, oplog_window())
        .await
        .map_err(space_error)?;
    notify_after_write(
        actor_store,
        blobstore_factory,
        server_config,
        transactor.keypair,
        transactor.space.clone(),
        did,
        space,
        &commit,
    )
    .await;
    Ok(commit)
}

/// Update the writer set and queue notifyWrite deliveries. The authority's
/// space host is auto-registered as a subscriber on the first write into a
/// shared space; a local authority is updated in place instead.
#[allow(clippy::too_many_arguments)]
async fn notify_after_write(
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    server_config: &State<ServerConfig>,
    keypair: Keypair,
    own_space_store: SpaceStore,
    did: &str,
    space: &SpaceId,
    commit: &SpaceCommitResult,
) {
    let mut endpoints: Vec<String> = Vec::new();
    let now = rsky_common::now();
    // The authority's view of the space: writer set + registered syncers.
    if actor_store.exists(&space.authority).await.unwrap_or(false) {
        match actor_store
            .read(
                space.authority.clone(),
                blobstore_factory.blobstore(space.authority.clone()),
            )
            .await
        {
            Ok(authority_reader) => {
                if let Err(error) = authority_reader
                    .space
                    .upsert_writer(
                        &space.uri(),
                        did,
                        &commit.rev,
                        Some(hex::encode(commit.hash)),
                    )
                    .await
                {
                    tracing::warn!(%error, "failed to update writer set");
                }
                match authority_reader
                    .space
                    .host_notify_endpoints(&space.uri(), &now)
                    .await
                {
                    Ok(host_endpoints) => endpoints.extend(host_endpoints),
                    Err(error) => tracing::warn!(%error, "failed to load host registrations"),
                }
            }
            Err(error) => tracing::warn!(%error, "failed to open authority store"),
        }
    }
    match own_space_store
        .repo_notify_endpoints(&space.uri(), &now)
        .await
    {
        Ok(repo_endpoints) => endpoints.extend(repo_endpoints),
        Err(error) => tracing::warn!(%error, "failed to load repo notify registrations"),
    }
    endpoints.sort();
    endpoints.dedup();

    let remote_authority =
        space.authority != did && !actor_store.exists(&space.authority).await.unwrap_or(false);
    let ctx = NotifyWriteTask {
        keypair,
        own_space_store,
        did: did.to_string(),
        space: space.clone(),
        rev: commit.rev.clone(),
        endpoints,
        plc_url: server_config.identity.plc_url.clone(),
        auto_register_authority: remote_authority,
    };
    actor_store
        .background_queue
        .add(async move { ctx.run().await });
}

struct NotifyWriteTask {
    keypair: Keypair,
    own_space_store: SpaceStore,
    did: String,
    space: SpaceId,
    rev: String,
    endpoints: Vec<String>,
    plc_url: String,
    auto_register_authority: bool,
}

impl NotifyWriteTask {
    async fn run(mut self) -> Result<()> {
        if self.auto_register_authority {
            match resolve_space_host_endpoint(&self.plc_url, &self.space.authority).await {
                Ok(endpoint) => {
                    let expires_at = format_expiry(&notify_expiry());
                    if let Err(error) = self
                        .own_space_store
                        .register_repo_notify(&self.space.uri(), &endpoint, &expires_at)
                        .await
                    {
                        tracing::warn!(%error, "failed to auto-register space host");
                    }
                    if !self.endpoints.contains(&endpoint) {
                        self.endpoints.push(endpoint);
                    }
                }
                Err(error) => {
                    tracing::debug!(%error, authority = %self.space.authority,
                        "could not resolve space host for auto-registration");
                }
            }
        }
        let body = serde_json::json!({
            "space": self.space.uri(),
            "did": self.did,
            "rev": self.rev,
        });
        deliver_notifications(
            &self.keypair,
            &self.did,
            &self.space.authority,
            NOTIFY_WRITE_LXM,
            &self.endpoints,
            &body,
        )
        .await;
        Ok(())
    }
}

/// Resolve the authority's `#atproto_space_host` service endpoint, falling
/// back to its `#atproto_pds` endpoint (spec §Space authority).
pub async fn resolve_space_host_endpoint(plc_url: &str, authority: &str) -> Result<String> {
    use rsky_identity::did::did_resolver::DidResolver;
    use rsky_identity::types::{DidResolverOpts, MemoryCache};
    use std::sync::Arc;

    let resolver = DidResolver::new(DidResolverOpts {
        timeout: None,
        plc_url: Some(plc_url.to_string()),
        did_cache: Arc::new(MemoryCache::new(None, None)),
    });
    let doc = resolver
        .ensure_resolve(&authority.to_string(), None)
        .await?;
    let services = doc
        .service
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("no services in DID document for {authority}"))?;
    for fragment in ["atproto_space_host", "atproto_pds"] {
        if let Some(service) = services
            .iter()
            .find(|s| s.id.rsplit_once('#').map(|(_, f)| f) == Some(fragment) || s.id == fragment)
        {
            return Ok(service.service_endpoint.clone());
        }
    }
    anyhow::bail!("no space host endpoint in DID document for {authority}")
}

/// POST a signed, method-bound notification to each endpoint, best-effort.
pub async fn deliver_notifications(
    keypair: &Keypair,
    iss: &str,
    aud: &str,
    lxm: &str,
    endpoints: &[String],
    body: &serde_json::Value,
) {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("reqwest client");
    for endpoint in endpoints {
        let token = match mint_space_service_token(keypair, iss, aud, lxm) {
            Ok(token) => token,
            Err(error) => {
                tracing::warn!(%error, "failed to mint notification token");
                return;
            }
        };
        let url = format!("{}/xrpc/{lxm}", endpoint.trim_end_matches('/'));
        match client.post(&url).bearer_auth(token).json(body).send().await {
            Ok(response) if response.status().is_success() => {
                tracing::debug!(%url, "notification delivered");
            }
            Ok(response) => {
                tracing::debug!(%url, status = %response.status(), "notification rejected");
            }
            Err(error) => {
                tracing::debug!(%url, %error, "notification delivery failed");
            }
        }
    }
}

/// Queue best-effort notifySpaceDeleted deliveries (spec §Space deletion).
pub fn queue_space_deleted_notifications(
    actor_store: &State<ActorStore>,
    keypair: Keypair,
    authority: String,
    space_uri: String,
    endpoints: Vec<String>,
) {
    actor_store.background_queue.add(async move {
        let body = serde_json::json!({ "space": space_uri });
        deliver_notifications(
            &keypair,
            &authority,
            &authority,
            NOTIFY_SPACE_DELETED_LXM,
            &endpoints,
            &body,
        )
        .await;
        Ok(())
    });
}

/// Basic rkey/skey syntax check (charset shared with public record keys).
pub fn valid_key_part(part: &str, max_len: usize) -> bool {
    !part.is_empty()
        && part.len() <= max_len
        && part != "."
        && part != ".."
        && part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '~'))
}

pub fn valid_nsid(s: &str) -> bool {
    s.matches('.').count() >= 2
        && s.len() <= 317
        && s.split('.').all(|seg| {
            !seg.is_empty() && seg.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}
