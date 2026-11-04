//! The routes the production image adds beside the reference PDS: the
//! on-demand TLS check its edge asks before issuing a certificate, and the
//! handle routes it rewrites `/.well-known/atproto-did` and
//! `resolveHandle` into. Same paths, same answers, same status codes.

use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::config::ServerConfig;
use crate::well_known::HostHeader;
use crate::SharedIdResolver;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::State;
use serde::Serialize;

#[derive(Serialize)]
pub struct TlsCheckOutput {
    pub success: bool,
}

#[derive(Serialize)]
pub struct ResolvedHandle {
    pub did: String,
}

/// Whether a certificate may be issued for `domain`: the server's own host,
/// or any handle on a domain it serves, whether or not the account exists
/// yet, so the handle works the moment the account is created.
#[tracing::instrument(skip_all)]
#[rocket::get("/tls-check?<domain>")]
pub async fn tls_check(
    domain: Option<String>,
    cfg: &State<ServerConfig>,
    account_manager: AccountManager,
) -> Result<Json<TlsCheckOutput>, ApiError> {
    let Some(domain) = domain.filter(|domain| !domain.is_empty()) else {
        return Err(ApiError::InvalidRequest(
            "bad or missing domain query param".to_string(),
        ));
    };
    if domain == cfg.service.hostname {
        return Ok(Json(TlsCheckOutput { success: true }));
    }
    if !cfg.identity.is_hosted_handle(&domain) {
        return Err(ApiError::InvalidRequest(
            "handles are not provided on this domain".to_string(),
        ));
    }
    if account_manager.get_account(&domain, None).await?.is_none() {
        tracing::info!(domain, "tls-check: domain valid, account not yet created");
    }
    Ok(Json(TlsCheckOutput { success: true }))
}

/// The DID behind a hosted handle, as plain text, taken from the query or
/// the request's host.
#[tracing::instrument(skip_all)]
#[rocket::get("/custom-well-known-atproto-did?<handle>")]
pub async fn custom_well_known_did(
    handle: Option<String>,
    host: Option<HostHeader>,
    cfg: &State<ServerConfig>,
    account_manager: AccountManager,
) -> Result<String, status::Custom<String>> {
    let handle = handle
        .filter(|handle| !handle.is_empty())
        .or_else(|| host.map(|host| host.0))
        .ok_or_else(user_not_found)?;
    if !cfg.identity.is_hosted_handle(&handle) {
        return Err(user_not_found());
    }
    let lookup = account_manager
        .get_account(&handle, None)
        .await
        .map(|account| account.map(|account| account.did));
    well_known_outcome(&handle, lookup)
}

fn user_not_found() -> status::Custom<String> {
    status::Custom(Status::NotFound, "User not found".to_string())
}

fn well_known_outcome(
    handle: &str,
    lookup: anyhow::Result<Option<String>>,
) -> Result<String, status::Custom<String>> {
    match lookup {
        Ok(Some(did)) => Ok(did),
        Ok(None) => Err(user_not_found()),
        Err(err) => {
            tracing::error!(?err, handle, "failed to get account for well-known DID");
            Err(status::Custom(
                Status::InternalServerError,
                "Internal Server Error".to_string(),
            ))
        }
    }
}

/// Resolves a handle: a local account answers at once, a hosted handle
/// with no account is an error, and anything else is resolved through the
/// network.
#[tracing::instrument(skip_all)]
#[rocket::get("/custom-resolve-handle?<handle>")]
pub async fn custom_resolve_handle(
    handle: Option<String>,
    cfg: &State<ServerConfig>,
    id_resolver: &State<SharedIdResolver>,
    account_manager: AccountManager,
) -> Result<Json<ResolvedHandle>, ApiError> {
    let Some(handle) = handle.filter(|handle| !handle.is_empty()) else {
        return Err(ApiError::InvalidRequest(
            "bad or missing handle query param".to_string(),
        ));
    };
    let normalized = handle.trim().to_ascii_lowercase();
    if let Some(account) = account_manager.get_account(&normalized, None).await? {
        return Ok(Json(ResolvedHandle { did: account.did }));
    }
    if cfg.identity.is_hosted_handle(&normalized) {
        return Err(ApiError::InvalidRequest(
            "Unable to resolve handle".to_string(),
        ));
    }
    let resolved = {
        let mut lock = id_resolver.id_resolver.write().await;
        lock.handle.resolve(&normalized).await.ok().flatten()
    };
    resolution_outcome(resolved)
}

fn resolution_outcome(resolved: Option<String>) -> Result<Json<ResolvedHandle>, ApiError> {
    match resolved {
        Some(did) => Ok(Json(ResolvedHandle { did })),
        None => Err(ApiError::InvalidRequest(
            "Unable to resolve handle".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn well_known_answers_match_the_production_image() {
        assert_eq!(
            well_known_outcome("a.test", Ok(Some("did:plc:a".to_string()))).unwrap(),
            "did:plc:a"
        );
        let missing = well_known_outcome("a.test", Ok(None)).unwrap_err();
        assert_eq!(
            (missing.0, missing.1.as_str()),
            (Status::NotFound, "User not found")
        );
        let failed = well_known_outcome("a.test", Err(anyhow::anyhow!("locked"))).unwrap_err();
        assert_eq!(
            (failed.0, failed.1.as_str()),
            (Status::InternalServerError, "Internal Server Error")
        );
    }

    #[test]
    fn resolution_answers_match_the_production_image() {
        assert_eq!(
            resolution_outcome(Some("did:plc:a".to_string()))
                .unwrap()
                .did,
            "did:plc:a"
        );
        assert!(matches!(
            resolution_outcome(None),
            Err(ApiError::InvalidRequest(message)) if message == "Unable to resolve handle"
        ));
    }
}
