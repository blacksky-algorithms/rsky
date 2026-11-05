//! WebFinger (RFC 7033) endpoint for AT Protocol identity discovery.
//!
//! Provides a standards-compliant WebFinger implementation that enables
//! discovery of AT Protocol accounts using DIDs, acct-URIs, and AT Protocol URIs.

use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::config::ServerConfig;
use crate::SharedIdResolver;
use rocket::http::Header;
use rocket::response::Responder;
use rocket::serde::json::Json;
use rocket::{Request, State};
use serde::{Deserialize, Serialize};

/// WebFinger response with CORS headers for cross-origin discovery.
///
/// Wraps the JSON response and adds appropriate CORS headers to allow
/// browser-based clients to query WebFinger from different origins.
pub struct CorsWebFingerResponse(Json<WebFingerResponse>);

impl<'r> Responder<'r, 'static> for CorsWebFingerResponse {
    fn respond_to(self, req: &'r Request<'_>) -> rocket::response::Result<'static> {
        let mut response = self.0.respond_to(req)?;

        // Add CORS headers for cross-origin discovery
        // WebFinger is a public discovery protocol, so we allow requests from any origin
        response.set_header(Header::new("Access-Control-Allow-Origin", "*"));
        response.set_header(Header::new("Access-Control-Allow-Methods", "GET"));
        response.set_header(Header::new("Access-Control-Allow-Headers", "Content-Type"));

        Ok(response)
    }
}

/// WebFinger response structure per RFC 7033.
#[derive(Debug, Serialize, Deserialize)]
pub struct WebFingerResponse {
    pub subject: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aliases: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<std::collections::HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<WebFingerLink>>,
}

/// WebFinger link relation per RFC 7033.
#[derive(Debug, Serialize, Deserialize)]
pub struct WebFingerLink {
    pub rel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub titles: Option<std::collections::HashMap<String, String>>,
}

/// Represents the different types of resource identifiers WebFinger can accept.
/// All must be valid URIs per RFC 7033 Section 4.2.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ResourceIdentifier {
    Did(String),                              // did:plc:...
    AcctUri { handle: String, host: String }, // acct:handle@host
    AtUri(String),                            // at://...
}

/// Parse a WebFinger resource parameter into a structured identifier.
///
/// Per RFC 7033 Section 4.2, the resource parameter MUST be a URI.
/// This function validates the URI scheme and structure.
///
/// # Supported Schemes
/// - `did:` - Decentralized Identifiers
/// - `acct:` - RFC 7565 account URIs (acct:user@host)
/// - `at://` - AT Protocol URIs
///
/// # Errors
/// Returns `ApiError::InvalidRequest` if:
/// - The resource is not a URI (no recognized scheme)
/// - The URI structure is invalid (e.g., malformed acct-URI)
///
/// # Examples
/// ```ignore
/// let did = parse_resource_identifier("did:plc:abc123")?;
/// let acct = parse_resource_identifier("acct:alice@example.com")?;
/// let at_uri = parse_resource_identifier("at://alice.example.com")?;
/// ```
fn parse_resource_identifier(resource: &str) -> Result<ResourceIdentifier, ApiError> {
    // Validate input
    if resource.is_empty() {
        return Err(ApiError::InvalidRequest(
            "resource parameter cannot be empty".to_string(),
        ));
    }

    // Prevent abuse: limit resource parameter length
    if resource.len() > 16384 {
        return Err(ApiError::InvalidRequest(
            "resource parameter too long (max 16384 characters)".to_string(),
        ));
    }

    // Parse by URI scheme
    if resource.starts_with("did:") {
        parse_did_uri(resource)
    } else if resource.starts_with("acct:") {
        parse_acct_uri(resource)
    } else if resource.starts_with("at://") {
        parse_at_uri(resource)
    } else {
        // REJECT: RFC 7033 requires resource to be a URI
        Err(ApiError::InvalidRequest(format!(
            "resource must be a URI with a valid scheme (acct:, did:, at://). Got: '{}'",
            resource
        )))
    }
}

/// Parse a DID URI (did:method:identifier)
fn parse_did_uri(resource: &str) -> Result<ResourceIdentifier, ApiError> {
    // Basic validation: must have at least "did:x:y" structure
    let parts: Vec<&str> = resource.splitn(3, ':').collect();
    if parts.len() < 3 || parts[1].is_empty() || parts[2].is_empty() {
        return Err(ApiError::InvalidRequest(format!(
            "Invalid DID format. Expected: did:method:identifier. Got: '{}'",
            resource
        )));
    }
    Ok(ResourceIdentifier::Did(resource.to_string()))
}

/// Parse an acct-URI (acct:user@host)
fn parse_acct_uri(resource: &str) -> Result<ResourceIdentifier, ApiError> {
    let acct = resource.strip_prefix("acct:").unwrap();

    if acct.is_empty() {
        return Err(ApiError::InvalidRequest(
            "Invalid acct-URI: missing user@host part".to_string(),
        ));
    }

    let parts: Vec<&str> = acct.split('@').collect();
    if parts.len() != 2 {
        return Err(ApiError::InvalidRequest(format!(
            "Invalid acct-URI format. Expected: acct:user@host. Got: '{}'",
            resource
        )));
    }

    let handle = parts[0];
    let host = parts[1];

    if handle.is_empty() {
        return Err(ApiError::InvalidRequest(
            "Invalid acct-URI: user part cannot be empty".to_string(),
        ));
    }

    if host.is_empty() {
        return Err(ApiError::InvalidRequest(
            "Invalid acct-URI: host part cannot be empty".to_string(),
        ));
    }

    Ok(ResourceIdentifier::AcctUri {
        handle: handle.to_string(),
        host: host.to_string(),
    })
}

/// Parse an AT Protocol URI (at://...)
fn parse_at_uri(resource: &str) -> Result<ResourceIdentifier, ApiError> {
    let path = resource.strip_prefix("at://").unwrap();

    if path.is_empty() {
        return Err(ApiError::InvalidRequest(
            "Invalid AT URI: missing identifier after at://".to_string(),
        ));
    }

    // AT URIs can contain DIDs or handles - we accept both
    // More detailed parsing can be added here if needed
    Ok(ResourceIdentifier::AtUri(resource.to_string()))
}

/// WebFinger endpoint handler.
///
/// Returns JSON Resource Descriptor (JRD) for a given resource identifier.
/// Includes CORS headers to enable cross-origin discovery.
#[tracing::instrument(skip_all)]
#[rocket::get("/.well-known/webfinger?<resource>")]
pub async fn webfinger(
    resource: String,
    cfg: &State<ServerConfig>,
    id_resolver: &State<SharedIdResolver>,
    account_manager: AccountManager,
) -> Result<CorsWebFingerResponse, ApiError> {
    // Parse and resolve the resource
    let (did, handle) = resolve_resource(&resource, cfg, id_resolver, &account_manager).await?;

    // Build the WebFinger response
    let response = build_webfinger_response(did, handle, cfg)?;

    Ok(CorsWebFingerResponse(Json(response)))
}

/// Check if a host matches the server's hostname or service handle domains.
///
/// This function implements proper domain matching that handles:
/// - Exact hostname match
/// - Domains with leading dots (e.g., ".rsky.com" matches "sub.rsky.com" or "rsky.com")
/// - Domains without leading dots (e.g., "example.com" matches "sub.example.com" but not "notexample.com")
fn is_valid_host(host: &str, hostname: &str, service_handle_domains: &[String]) -> bool {
    if host == hostname {
        return true;
    }

    service_handle_domains.iter().any(|domain| {
        if let Some(bare_domain) = domain.strip_prefix('.') {
            // For domains like ".rsky.com", match "rsky.com" or "*.rsky.com"
            host == bare_domain || host.ends_with(domain.as_str())
        } else {
            // For domains without leading dot, exact match or subdomain
            host == domain.as_str() || host.ends_with(&format!(".{}", domain))
        }
    })
}

/// Resolve a resource identifier to a DID and optional handle.
///
/// Validates that the resource is hosted on this PDS and the account is active.
async fn resolve_resource(
    resource: &str,
    cfg: &ServerConfig,
    _id_resolver: &SharedIdResolver, // Reserved for future remote handle resolution
    account_manager: &AccountManager,
) -> Result<(String, Option<String>), ApiError> {
    let identifier = parse_resource_identifier(resource)?;

    match identifier {
        ResourceIdentifier::Did(did) => {
            // Verify this DID is hosted locally
            match account_manager.get_account(&did, None).await? {
                Some(account) => {
                    // Check if account is available
                    if account.deactivated_at.is_some() || account.takedown_ref.is_some() {
                        return Err(ApiError::AccountNotFound);
                    }
                    Ok((did, account.handle))
                }
                None => Err(ApiError::AccountNotFound),
            }
        }
        ResourceIdentifier::AcctUri { handle, host } => {
            // Verify host matches this PDS
            // Use proper domain validation that handles domains with leading dots (e.g., ".rsky.com")
            if !is_valid_host(
                &host,
                &cfg.service.hostname,
                &cfg.identity.service_handle_domains,
            ) {
                return Err(ApiError::InvalidRequest(
                    "Host does not match this server".to_string(),
                ));
            }
            // Resolve handle to DID
            match account_manager.get_account(&handle, None).await? {
                Some(account) => {
                    if account.deactivated_at.is_some() || account.takedown_ref.is_some() {
                        return Err(ApiError::AccountNotFound);
                    }
                    Ok((account.did, Some(handle)))
                }
                None => Err(ApiError::AccountNotFound),
            }
        }
        ResourceIdentifier::AtUri(uri) => {
            // Parse at:// URI and extract DID or handle
            let stripped = uri.strip_prefix("at://").unwrap_or(&uri);
            if stripped.starts_with("did:") {
                // Resolve as DID
                match account_manager.get_account(stripped, None).await? {
                    Some(account) => {
                        if account.deactivated_at.is_some() || account.takedown_ref.is_some() {
                            return Err(ApiError::AccountNotFound);
                        }
                        Ok((account.did, account.handle))
                    }
                    None => Err(ApiError::AccountNotFound),
                }
            } else {
                // Assume handle - extract just the authority (before any path)
                let authority = stripped.split('/').next().unwrap_or(stripped);
                match account_manager.get_account(authority, None).await? {
                    Some(account) => {
                        if account.deactivated_at.is_some() || account.takedown_ref.is_some() {
                            return Err(ApiError::AccountNotFound);
                        }
                        Ok((account.did, Some(authority.to_string())))
                    }
                    None => Err(ApiError::AccountNotFound),
                }
            }
        }
    }
}

/// Build a WebFinger response from account information.
fn build_webfinger_response(
    did: String,
    handle: Option<String>,
    cfg: &ServerConfig,
) -> Result<WebFingerResponse, ApiError> {
    let hostname = &cfg.service.hostname;
    let public_url = &cfg.service.public_url;

    // Subject: prefer acct-URI if handle is known
    let subject = match &handle {
        Some(h) => format!("acct:{}@{}", h, hostname),
        None => did.clone(),
    };

    // Aliases
    let mut aliases = vec![did.clone()];
    if let Some(h) = &handle {
        aliases.push(format!("at://{}", h));
        aliases.push(format!("at://{}", did));
    }

    // Properties
    // NOTE: The property namespace URIs (https://atproto.com/ns/*) are UNOFFICIAL
    // and not defined by AT Protocol. They follow RFC 7033 best practice of using
    // URIs to avoid naming collisions, but these URIs do not resolve to actual resources.
    let mut properties = std::collections::HashMap::new();
    properties.insert("https://atproto.com/ns/did".to_string(), did.clone());
    if let Some(h) = &handle {
        properties.insert("https://atproto.com/ns/handle".to_string(), h.clone());
    }

    // Links
    let mut links = vec![];

    // DID document link (PLC directory)
    links.push(WebFingerLink {
        rel: "self".to_string(),
        r#type: Some("application/did+ld+json".to_string()),
        href: Some(format!("https://plc.directory/{}", did)),
        titles: None,
    });

    // Profile pages (if app view is configured)
    if let Some(app_view) = &cfg.bsky_app_view {
        // DID-based profile (stable, for ActivityPub/federation compatibility)
        links.push(WebFingerLink {
            rel: "http://webfinger.net/rel/profile-page".to_string(),
            r#type: Some("application/activity+json".to_string()),
            href: Some(format!("{}/profile/{}", app_view.url, did)),
            titles: None,
        });

        // Handle-based profile (user-friendly, if handle is known)
        if let Some(h) = &handle {
            links.push(WebFingerLink {
                rel: "http://webfinger.net/rel/profile-page".to_string(),
                r#type: Some("text/html".to_string()),
                href: Some(format!("{}/profile/{}", app_view.url, h)),
                titles: None,
            });
        }
    }

    // Repository export
    links.push(WebFingerLink {
        rel: "alternate".to_string(),
        r#type: Some("application/vnd.ipld.car".to_string()),
        href: Some(format!(
            "{}/xrpc/com.atproto.sync.getRepo?did={}",
            public_url, did
        )),
        titles: None,
    });

    // Identity resolution endpoint
    links.push(WebFingerLink {
        rel: "https://atproto.com/rels/did-doc".to_string(),
        r#type: Some("application/did+ld+json".to_string()),
        href: Some(format!(
            "{}/xrpc/com.atproto.identity.resolveIdentity?identifier={}",
            public_url, did
        )),
        titles: None,
    });

    Ok(WebFingerResponse {
        subject,
        aliases: Some(aliases),
        properties: Some(properties),
        links: Some(links),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // DID URI Parsing Tests
    // ============================================================================

    #[test]
    fn parses_valid_did_plc() {
        let resource = "did:plc:abc123def456";
        let parsed = parse_resource_identifier(resource).unwrap();
        match parsed {
            ResourceIdentifier::Did(did) => {
                assert_eq!(did, "did:plc:abc123def456");
            }
            _ => panic!("Expected Did variant"),
        }
    }

    #[test]
    fn parses_valid_did_web() {
        let resource = "did:web:example.com";
        let parsed = parse_resource_identifier(resource).unwrap();
        assert!(matches!(parsed, ResourceIdentifier::Did(_)));
    }

    #[test]
    fn parses_valid_did_key() {
        let resource = "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK";
        let parsed = parse_resource_identifier(resource).unwrap();
        assert!(matches!(parsed, ResourceIdentifier::Did(_)));
    }

    #[test]
    fn rejects_malformed_did_no_method() {
        let resource = "did::identifier";
        assert!(parse_resource_identifier(resource).is_err());
    }

    #[test]
    fn rejects_malformed_did_no_identifier() {
        let resource = "did:plc:";
        assert!(parse_resource_identifier(resource).is_err());
    }

    #[test]
    fn rejects_malformed_did_only_method() {
        let resource = "did:plc";
        assert!(parse_resource_identifier(resource).is_err());
    }

    // ============================================================================
    // acct-URI Parsing Tests
    // ============================================================================

    #[test]
    fn parses_valid_acct_uri() {
        let resource = "acct:alice.test@example.com";
        let parsed = parse_resource_identifier(resource).unwrap();
        match parsed {
            ResourceIdentifier::AcctUri { handle, host } => {
                assert_eq!(handle, "alice.test");
                assert_eq!(host, "example.com");
            }
            _ => panic!("Expected AcctUri variant"),
        }
    }

    #[test]
    fn parses_acct_uri_with_subdomain() {
        let resource = "acct:user@sub.domain.example.com";
        let parsed = parse_resource_identifier(resource).unwrap();
        match parsed {
            ResourceIdentifier::AcctUri { handle, host } => {
                assert_eq!(handle, "user");
                assert_eq!(host, "sub.domain.example.com");
            }
            _ => panic!("Expected AcctUri variant"),
        }
    }

    #[test]
    fn parses_acct_uri_with_dots_in_handle() {
        let resource = "acct:alice.bob.test@example.com";
        let parsed = parse_resource_identifier(resource).unwrap();
        match parsed {
            ResourceIdentifier::AcctUri { handle, host } => {
                assert_eq!(handle, "alice.bob.test");
                assert_eq!(host, "example.com");
            }
            _ => panic!("Expected AcctUri variant"),
        }
    }

    #[test]
    fn rejects_acct_uri_no_at_sign() {
        let resource = "acct:invalid";
        let err = parse_resource_identifier(resource).unwrap_err();
        match err {
            ApiError::InvalidRequest(msg) => {
                assert!(msg.contains("acct:user@host"));
            }
            _ => panic!("Expected InvalidRequest error"),
        }
    }

    #[test]
    fn rejects_acct_uri_multiple_at_signs() {
        // Only first @ is split point - should reject multiple
        let resource = "acct:alice@example@com";
        let err = parse_resource_identifier(resource).unwrap_err();
        match err {
            ApiError::InvalidRequest(msg) => {
                assert!(msg.contains("acct:user@host"));
            }
            _ => panic!("Expected InvalidRequest error"),
        }
    }

    #[test]
    fn rejects_acct_uri_empty_user() {
        let resource = "acct:@example.com";
        let err = parse_resource_identifier(resource).unwrap_err();
        match err {
            ApiError::InvalidRequest(msg) => {
                assert!(msg.contains("user part cannot be empty"));
            }
            _ => panic!("Expected InvalidRequest error"),
        }
    }

    #[test]
    fn rejects_acct_uri_empty_host() {
        let resource = "acct:alice@";
        let err = parse_resource_identifier(resource).unwrap_err();
        match err {
            ApiError::InvalidRequest(msg) => {
                assert!(msg.contains("host part cannot be empty"));
            }
            _ => panic!("Expected InvalidRequest error"),
        }
    }

    #[test]
    fn rejects_acct_uri_missing_user_host() {
        let resource = "acct:";
        let err = parse_resource_identifier(resource).unwrap_err();
        match err {
            ApiError::InvalidRequest(msg) => {
                assert!(msg.contains("missing user@host"));
            }
            _ => panic!("Expected InvalidRequest error"),
        }
    }

    // ============================================================================
    // AT Protocol URI Parsing Tests
    // ============================================================================

    #[test]
    fn parses_at_uri_with_handle() {
        let resource = "at://alice.test";
        let parsed = parse_resource_identifier(resource).unwrap();
        match parsed {
            ResourceIdentifier::AtUri(uri) => {
                assert_eq!(uri, "at://alice.test");
            }
            _ => panic!("Expected AtUri variant"),
        }
    }

    #[test]
    fn parses_at_uri_with_did() {
        let resource = "at://did:plc:abc123";
        let parsed = parse_resource_identifier(resource).unwrap();
        assert!(matches!(parsed, ResourceIdentifier::AtUri(_)));
    }

    #[test]
    fn parses_at_uri_with_path() {
        let resource = "at://alice.test/app.bsky.feed.post/3k2yihcrj5s2s";
        let parsed = parse_resource_identifier(resource).unwrap();
        assert!(matches!(parsed, ResourceIdentifier::AtUri(_)));
    }

    #[test]
    fn rejects_at_uri_empty_path() {
        let resource = "at://";
        let err = parse_resource_identifier(resource).unwrap_err();
        match err {
            ApiError::InvalidRequest(msg) => {
                assert!(msg.contains("missing identifier after at://"));
            }
            _ => panic!("Expected InvalidRequest error"),
        }
    }

    // ============================================================================
    // General Validation Tests
    // ============================================================================

    #[test]
    fn rejects_empty_string() {
        let resource = "";
        let err = parse_resource_identifier(resource).unwrap_err();
        match err {
            ApiError::InvalidRequest(msg) => {
                assert!(msg.contains("cannot be empty"));
            }
            _ => panic!("Expected InvalidRequest error"),
        }
    }

    #[test]
    fn rejects_excessively_long_resource() {
        // Create a resource string longer than 16384 characters
        let long_did = format!("did:plc:{}", "a".repeat(16400));
        let err = parse_resource_identifier(&long_did).unwrap_err();
        match err {
            ApiError::InvalidRequest(msg) => {
                assert!(msg.contains("too long"));
                assert!(msg.contains("16384"));
            }
            _ => panic!("Expected InvalidRequest error"),
        }
    }

    #[test]
    fn accepts_resource_at_max_length() {
        // Verify we accept resources at exactly the limit
        let max_length_did = format!("did:plc:{}", "a".repeat(16384 - 8)); // "did:plc:" = 8 chars
        let result = parse_resource_identifier(&max_length_did);
        assert!(result.is_ok(), "Should accept resource at max length");
    }

    #[test]
    fn rejects_bare_handles() {
        // RFC 7033 requires URIs - bare handles are not valid
        let resource = "alice.test";
        let err = parse_resource_identifier(resource).unwrap_err();
        match err {
            ApiError::InvalidRequest(msg) => {
                assert!(msg.contains("must be a URI"));
                assert!(msg.contains("acct:"));
                assert!(msg.contains("did:"));
                assert!(msg.contains("at://"));
            }
            _ => panic!("Expected InvalidRequest error"),
        }
    }

    #[test]
    fn rejects_bare_email_format() {
        // Must use acct: prefix
        let resource = "alice@example.com";
        let err = parse_resource_identifier(resource).unwrap_err();
        match err {
            ApiError::InvalidRequest(msg) => {
                assert!(msg.contains("must be a URI"));
            }
            _ => panic!("Expected InvalidRequest error"),
        }
    }

    #[test]
    fn rejects_https_uris() {
        // HTTPS URIs not supported - PDS has no profile pages
        let resource = "https://example.com/profile/alice";
        let err = parse_resource_identifier(resource).unwrap_err();
        match err {
            ApiError::InvalidRequest(msg) => {
                assert!(msg.contains("must be a URI"));
                // Verify we don't suggest https:// or http:// as valid schemes
                assert!(msg.contains("acct:"));
                assert!(msg.contains("did:"));
                assert!(msg.contains("at://"));
                // The error message should list valid schemes but not suggest https
                let valid_schemes_part = msg.split("Got:").next().unwrap();
                assert!(!valid_schemes_part.contains("https://"));
                assert!(!valid_schemes_part.contains("http://"));
            }
            _ => panic!("Expected InvalidRequest error"),
        }
    }

    #[test]
    fn rejects_http_uris() {
        let resource = "http://example.com/profile/alice";
        assert!(parse_resource_identifier(resource).is_err());
    }

    #[test]
    fn rejects_unknown_scheme() {
        let resource = "ftp://example.com";
        let err = parse_resource_identifier(resource).unwrap_err();
        match err {
            ApiError::InvalidRequest(msg) => {
                assert!(msg.contains("must be a URI"));
            }
            _ => panic!("Expected InvalidRequest error"),
        }
    }

    #[test]
    fn rejects_whitespace() {
        let resource = "  ";
        let err = parse_resource_identifier(resource).unwrap_err();
        match err {
            ApiError::InvalidRequest(msg) => {
                assert!(msg.contains("must be a URI"));
            }
            _ => panic!("Expected InvalidRequest error"),
        }
    }

    #[test]
    fn preserves_case_in_did() {
        // DIDs are case-sensitive
        let resource = "did:plc:AbC123";
        let parsed = parse_resource_identifier(resource).unwrap();
        match parsed {
            ResourceIdentifier::Did(did) => {
                assert_eq!(did, "did:plc:AbC123");
            }
            _ => panic!("Expected Did variant"),
        }
    }

    #[test]
    fn preserves_case_in_acct_handle() {
        // Handles may be case-sensitive in parsing (normalized later)
        let resource = "acct:Alice.Test@Example.COM";
        let parsed = parse_resource_identifier(resource).unwrap();
        match parsed {
            ResourceIdentifier::AcctUri { handle, host } => {
                assert_eq!(handle, "Alice.Test");
                assert_eq!(host, "Example.COM");
            }
            _ => panic!("Expected AcctUri variant"),
        }
    }

    // ============================================================================
    // Error Message Quality Tests
    // ============================================================================

    #[test]
    fn error_messages_are_helpful() {
        let test_cases = vec![
            ("", "empty"),
            ("alice.test", "URI"),
            ("acct:invalid", "acct:user@host"),
            ("did:plc", "did:method:identifier"),
            ("at://", "missing identifier"),
        ];

        for (input, expected_fragment) in test_cases {
            let result = parse_resource_identifier(input);
            assert!(result.is_err(), "Should reject: {}", input);

            if let Err(ApiError::InvalidRequest(msg)) = result {
                assert!(
                    msg.contains(expected_fragment),
                    "Error for '{}' should contain '{}', got: {}",
                    input,
                    expected_fragment,
                    msg
                );
            }
        }
    }

    // ============================================================================
    // Host Validation Tests (Security Critical)
    // ============================================================================

    #[test]
    fn host_validation_exact_hostname_match() {
        let hostname = "pds.example.com";
        let service_domains = vec![];

        assert!(is_valid_host("pds.example.com", hostname, &service_domains));
        assert!(!is_valid_host(
            "other.example.com",
            hostname,
            &service_domains
        ));
        assert!(!is_valid_host("example.com", hostname, &service_domains));
    }

    #[test]
    fn host_validation_domain_with_leading_dot() {
        let hostname = "pds.example.com";
        let service_domains = vec![".rsky.com".to_string()];

        // Should match bare domain
        assert!(is_valid_host("rsky.com", hostname, &service_domains));

        // Should match subdomains
        assert!(is_valid_host("sub.rsky.com", hostname, &service_domains));
        assert!(is_valid_host(
            "deep.sub.rsky.com",
            hostname,
            &service_domains
        ));

        // Should NOT match similar but different domains
        assert!(!is_valid_host("notrsky.com", hostname, &service_domains));
        assert!(!is_valid_host(
            "rsky.com.evil.com",
            hostname,
            &service_domains
        ));
    }

    #[test]
    fn host_validation_domain_without_leading_dot() {
        let hostname = "pds.example.com";
        let service_domains = vec!["example.com".to_string()];

        // Should match exact domain
        assert!(is_valid_host("example.com", hostname, &service_domains));

        // Should match subdomains with proper dot prefix
        assert!(is_valid_host("sub.example.com", hostname, &service_domains));
        assert!(is_valid_host(
            "deep.sub.example.com",
            hostname,
            &service_domains
        ));

        // CRITICAL: Should NOT match domains that merely end with the string
        assert!(!is_valid_host("notexample.com", hostname, &service_domains));
        assert!(!is_valid_host("badexample.com", hostname, &service_domains));
        assert!(!is_valid_host(
            "example.com.evil.com",
            hostname,
            &service_domains
        ));
    }

    #[test]
    fn host_validation_multiple_service_domains() {
        let hostname = "pds.main.com";
        let service_domains = vec![
            ".rsky.com".to_string(),
            "example.com".to_string(),
            ".other.net".to_string(),
        ];

        // Should match hostname
        assert!(is_valid_host("pds.main.com", hostname, &service_domains));

        // Should match any of the service domains
        assert!(is_valid_host("rsky.com", hostname, &service_domains));
        assert!(is_valid_host("sub.rsky.com", hostname, &service_domains));
        assert!(is_valid_host("example.com", hostname, &service_domains));
        assert!(is_valid_host("sub.example.com", hostname, &service_domains));
        assert!(is_valid_host("other.net", hostname, &service_domains));
        assert!(is_valid_host("sub.other.net", hostname, &service_domains));

        // Should NOT match invalid domains
        assert!(!is_valid_host("notrsky.com", hostname, &service_domains));
        assert!(!is_valid_host("notexample.com", hostname, &service_domains));
        assert!(!is_valid_host("notother.net", hostname, &service_domains));
    }

    #[test]
    fn host_validation_regression_test_for_ends_with_bug() {
        // This test specifically covers the bug that was fixed
        let hostname = "pds.myserver.com";
        let service_domains = vec!["example.com".to_string()];

        // These should all FAIL - they merely end with "example.com" as a string
        // but are not valid subdomains
        assert!(!is_valid_host("notexample.com", hostname, &service_domains));
        assert!(!is_valid_host("badexample.com", hostname, &service_domains));
        assert!(!is_valid_host(
            "evilexample.com",
            hostname,
            &service_domains
        ));
        assert!(!is_valid_host("xxxexample.com", hostname, &service_domains));

        // These should PASS - they are valid
        assert!(is_valid_host("example.com", hostname, &service_domains));
        assert!(is_valid_host("sub.example.com", hostname, &service_domains));
    }

    #[test]
    fn host_validation_empty_service_domains() {
        let hostname = "pds.example.com";
        let service_domains = vec![];

        // Only hostname should match
        assert!(is_valid_host("pds.example.com", hostname, &service_domains));

        // Nothing else should match
        assert!(!is_valid_host("example.com", hostname, &service_domains));
        assert!(!is_valid_host("other.com", hostname, &service_domains));
    }

    #[test]
    fn host_validation_case_sensitivity() {
        // Note: HTTP hostnames are case-insensitive, but this tests the current implementation
        let hostname = "pds.example.com";
        let service_domains = vec!["example.com".to_string()];

        // Current implementation is case-sensitive (matches Rust string comparison)
        assert!(!is_valid_host("Example.com", hostname, &service_domains));
        assert!(!is_valid_host("EXAMPLE.COM", hostname, &service_domains));

        // This documents current behavior - in production, domains should be normalized
        // to lowercase before validation
    }

    // ============================================================================
    // Response Building Tests
    // ============================================================================

    /// Helper function to create a test ServerConfig
    fn create_test_config(app_view_url: Option<String>) -> ServerConfig {
        use crate::config::{
            ActorStoreConfig, BlobstoreConfig, CoreConfig, IdentityConfig, InvitesConfig,
            ServiceConfig, ServiceDbConfig, SubscriptionConfig,
        };

        ServerConfig {
            service: CoreConfig {
                port: 2583,
                hostname: "pds.example.com".to_string(),
                public_url: "https://pds.example.com".to_string(),
                did: "did:web:pds.example.com".to_string(),
                version: None,
                privacy_policy_url: None,
                terms_of_service_url: None,
                accepting_imports: false,
                blob_upload_limit: 52428800,
                contact_email_address: None,
                dev_mode: false,
                read_only: false,
                coexistence: false,
                write_allowlist_file: None,
                shutdown_grace_secs: 5,
                upload_spool_dir: "/tmp/uploads".to_string(),
                account_ui_sessions_since: None,
                account_ui_enabled: false,
            },
            mod_service: None,
            report_service: None,
            bsky_app_view: app_view_url.map(|url| ServiceConfig {
                url,
                did: "did:web:bsky.app".to_string(),
                cdn_url_pattern: None,
            }),
            subscription: SubscriptionConfig {
                max_buffer: 500,
                repo_backfill_limit_ms: 60000,
            },
            invites: InvitesConfig {
                required: false,
                interval: Some(604800000),
                epoch: Some(0),
            },
            identity: IdentityConfig {
                plc_url: "https://plc.directory".to_string(),
                resolver_timeout: 3000,
                cache_state_ttl: 300000,
                cache_max_ttl: 3600000,
                recovery_did_key: None,
                service_handle_domains: vec![],
                extra_handle_domains: vec![],
                handle_backup_name_servers: None,
                enable_did_doc_with_session: false,
            },
            crawlers: vec![],
            actor_store: ActorStoreConfig {
                directory: "/tmp/actors".to_string(),
                cache_size: 10,
            },
            service_db: ServiceDbConfig {
                account_db_location: ":memory:".to_string(),
                did_cache_db_location: ":memory:".to_string(),
                sequencer_db_location: ":memory:".to_string(),
                lifecycle_db_location: ":memory:".to_string(),
                repair_db_location: ":memory:".to_string(),
                blob_attempts_db_location: ":memory:".to_string(),
                blob_generations_db_location: ":memory:".to_string(),
                lock_dir: "/tmp/locks".to_string(),
            },
            blobstore: BlobstoreConfig::Disk {
                location: "/tmp/blobs".to_string(),
                tmp_location: None,
            },
        }
    }

    #[test]
    fn builds_response_with_handle() {
        let cfg = create_test_config(None);

        let did = "did:plc:abc123".to_string();
        let handle = Some("alice.example.com".to_string());

        let response = build_webfinger_response(did.clone(), handle.clone(), &cfg).unwrap();

        // Verify subject uses acct-URI when handle is present
        assert_eq!(response.subject, "acct:alice.example.com@pds.example.com");

        // Verify aliases
        let aliases = response.aliases.unwrap();
        assert!(aliases.contains(&did));
        assert!(aliases.contains(&"at://alice.example.com".to_string()));
        assert!(aliases.contains(&format!("at://{}", did)));

        // Verify properties
        let properties = response.properties.unwrap();
        assert_eq!(properties.get("https://atproto.com/ns/did"), Some(&did));
        assert_eq!(
            properties.get("https://atproto.com/ns/handle"),
            Some(&"alice.example.com".to_string())
        );

        // Verify links
        let links = response.links.unwrap();
        assert!(!links.is_empty());

        // Should have PLC directory link
        assert!(links.iter().any(
            |link| link.rel == "self" && link.href.as_ref().unwrap().contains("plc.directory")
        ));

        // Should have repo export link
        assert!(links.iter().any(|link| link.rel == "alternate"
            && link.r#type.as_ref().unwrap() == "application/vnd.ipld.car"));
    }

    #[test]
    fn builds_response_without_handle() {
        let cfg = create_test_config(None);

        let did = "did:plc:abc123".to_string();
        let handle = None;

        let response = build_webfinger_response(did.clone(), handle, &cfg).unwrap();

        // Verify subject falls back to DID when no handle
        assert_eq!(response.subject, did);

        // Verify aliases (should only have DID)
        let aliases = response.aliases.unwrap();
        assert_eq!(aliases.len(), 1);
        assert!(aliases.contains(&did));

        // Verify properties (should not have handle)
        let properties = response.properties.unwrap();
        assert_eq!(properties.get("https://atproto.com/ns/did"), Some(&did));
        assert!(!properties.contains_key("https://atproto.com/ns/handle"));
    }

    #[test]
    fn builds_response_with_app_view() {
        let cfg = create_test_config(Some("https://bsky.app".to_string()));

        let did = "did:plc:abc123".to_string();
        let handle = Some("alice.example.com".to_string());

        let response = build_webfinger_response(did.clone(), handle.clone(), &cfg).unwrap();

        let links = response.links.unwrap();

        // Should have DID-based profile page link
        assert!(links
            .iter()
            .any(|link| link.rel == "http://webfinger.net/rel/profile-page"
                && link.r#type.as_ref().unwrap() == "application/activity+json"
                && link.href.as_ref().unwrap().contains(&did)));

        // Should have handle-based profile page link
        assert!(links
            .iter()
            .any(|link| link.rel == "http://webfinger.net/rel/profile-page"
                && link.r#type.as_ref().unwrap() == "text/html"
                && link.href.as_ref().unwrap().contains("alice.example.com")));
    }

    #[test]
    fn builds_response_without_app_view() {
        let cfg = create_test_config(None);

        let did = "did:plc:abc123".to_string();
        let handle = Some("alice.example.com".to_string());

        let response = build_webfinger_response(did.clone(), handle, &cfg).unwrap();

        let links = response.links.unwrap();

        // Should NOT have profile page links
        assert!(!links
            .iter()
            .any(|link| link.rel == "http://webfinger.net/rel/profile-page"));

        // But should still have PLC and repo links
        assert!(links.iter().any(|link| link.rel == "self"));
        assert!(links.iter().any(|link| link.rel == "alternate"));
    }

    #[test]
    fn response_includes_all_required_links() {
        let cfg = create_test_config(None);

        let did = "did:plc:abc123".to_string();
        let handle = Some("alice.example.com".to_string());

        let response = build_webfinger_response(did.clone(), handle, &cfg).unwrap();

        let links = response.links.unwrap();

        // Self link (PLC directory)
        let self_link = links.iter().find(|link| link.rel == "self");
        assert!(self_link.is_some());
        assert_eq!(
            self_link.unwrap().r#type.as_ref().unwrap(),
            "application/did+ld+json"
        );
        assert_eq!(
            self_link.unwrap().href.as_ref().unwrap(),
            &format!("https://plc.directory/{}", did)
        );

        // Alternate link (repo export)
        let alternate_link = links.iter().find(|link| link.rel == "alternate");
        assert!(alternate_link.is_some());
        assert_eq!(
            alternate_link.unwrap().r#type.as_ref().unwrap(),
            "application/vnd.ipld.car"
        );

        // Identity resolution link
        let identity_link = links
            .iter()
            .find(|link| link.rel == "https://atproto.com/rels/did-doc");
        assert!(identity_link.is_some());
        assert_eq!(
            identity_link.unwrap().r#type.as_ref().unwrap(),
            "application/did+ld+json"
        );
    }
}
