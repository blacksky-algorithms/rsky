use crate::config::ServerConfig;
use crate::handle::errors::{Error, ErrorKind, Result};
use crate::SharedIdResolver;
use explicit_slurs::has_explicit_slur;
use reserved::RESERVED_SUBDOMAINS;
use rocket::State;
use rsky_syntax::handle::{is_valid_tld, normalize_and_ensure_valid_handle};

pub struct HandleValidationContext<'a> {
    pub server_config: &'a State<ServerConfig>,
    pub id_resolver: &'a State<SharedIdResolver>,
}

pub struct HandleValidationOpts {
    pub handle: String,
    pub did: Option<String>,
    pub allow_reserved: Option<bool>,
}

pub async fn normalize_and_validate_handle(
    opts: HandleValidationOpts,
    ctx: HandleValidationContext<'_>,
) -> Result<String> {
    // Base formatting validation
    let handle = base_normalize_and_validate(&opts.handle)?;

    // TLD validation
    if !is_valid_tld(&handle) {
        return Err(Error::new(
            ErrorKind::InvalidHandle,
            "Handle TLD is invalid or disallowed",
        ));
    }

    // Slur check
    if has_explicit_slur(&handle) {
        return Err(Error::new(
            ErrorKind::InvalidHandle,
            "Inappropriate language in handle",
        ));
    }

    let service_domains = &ctx.server_config.identity.service_handle_domains;
    if is_service_domain(&handle, service_domains) {
        // Verify constraints on a service domain
        ensure_handle_service_constraints(
            &handle,
            service_domains,
            opts.allow_reserved.unwrap_or(false),
        )?;
    } else {
        if opts.did.is_none() {
            return Err(Error::new(
                ErrorKind::UnsupportedDomain,
                "Not a supported handle domain",
            ));
        }

        // Verify resolution of a non-service domain
        let mut lock = ctx.id_resolver.id_resolver.write().await;
        match lock.handle.resolve(&handle).await.unwrap() {
            Some(resolved_did) => {
                if resolved_did != opts.did.unwrap() {
                    return Err(Error::new(
                        ErrorKind::InvalidHandle,
                        "External handle did not resolve to DID",
                    ));
                }
            }
            None => {
                return Err(Error::new(
                    ErrorKind::InvalidHandle,
                    "Id Resolver did not resolve to DID",
                ));
            }
        }
    }

    Ok(handle)
}

fn base_normalize_and_validate(handle: &str) -> Result<String> {
    match normalize_and_ensure_valid_handle(handle) {
        Ok(normalized) => Ok(normalized),
        Err(e) => Err(Error::new(ErrorKind::InvalidHandle, &e.to_string())),
    }
}

fn is_service_domain(handle: &str, available_user_domains: &[String]) -> bool {
    available_user_domains
        .iter()
        .any(|domain| handle.ends_with(domain))
}

fn ensure_handle_service_constraints(
    handle: &str,
    available_user_domains: &[String],
    allow_reserved: bool,
) -> Result<()> {
    let supported_domain = available_user_domains
        .iter()
        .find(|domain| handle.ends_with(*domain))
        .ok_or_else(|| Error::new(ErrorKind::InvalidHandle, "Invalid domain"))?;

    let front = handle[..handle.len() - supported_domain.len()].to_string();

    if front.contains('.') {
        return Err(Error::new(
            ErrorKind::InvalidHandle,
            "Invalid characters in handle",
        ));
    }

    if front.len() < 3 {
        return Err(Error::new(ErrorKind::InvalidHandle, "Handle too short"));
    }

    if front.len() > 18 {
        return Err(Error::new(ErrorKind::InvalidHandle, "Handle too long"));
    }

    if !allow_reserved && RESERVED_SUBDOMAINS.contains(front.as_str()) {
        return Err(Error::new(ErrorKind::HandleNotAvailable, "Reserved handle"));
    }

    Ok(())
}

pub mod errors;
pub mod explicit_slurs;
pub mod reserved;
