//! PDS (Personal Data Server) client for uploading blobs
//!
//! Handles uploading video blobs to users' PDS instances using the service auth token
//! provided by the client. The token contains the PDS DID as the audience, which allows
//! the video service to forward the token when uploading to the PDS.

use atrium_api::types::{BlobRef, TypedBlobRef};
use atrium_xrpc::{HttpClient, XrpcClient};
use atrium_xrpc::http::{Request, Response};
use atrium_xrpc::types::AuthorizationToken;
use atrium_xrpc_client::reqwest::ReqwestClient;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use bytes::Bytes;
use serde::Deserialize;
use serde_json::Value as JsonValue;
use tracing::{debug, info};

use crate::error::{Error, Result};

/// JWT claims from service auth token
#[derive(Debug, Deserialize)]
struct ServiceAuthClaims {
    /// Issuer (user's DID, signed by their PDS)
    iss: String,
    /// Audience (PDS DID - where the blob should be uploaded)
    aud: String,
    /// Subject (user's DID, optional)
    #[serde(default)]
    sub: Option<String>,
    /// Lexicon method
    #[serde(default)]
    #[allow(dead_code)]
    lxm: Option<String>,
}

/// Response from DID document resolution
#[derive(Debug, Deserialize)]
struct DidDocument {
    #[serde(default)]
    service: Vec<DidService>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DidService {
    id: String,
    #[serde(rename = "type")]
    service_type: String,
    service_endpoint: String,
}

/// Wrapper XRPC client that uses a bearer token for auth
struct AuthenticatedClient {
    token: String,
    inner: ReqwestClient,
}

impl AuthenticatedClient {
    fn new(base_uri: &str, token: String) -> Self {
        Self {
            token,
            inner: ReqwestClient::new(base_uri),
        }
    }
}

impl HttpClient for AuthenticatedClient {
    async fn send_http(
        &self,
        request: Request<Vec<u8>>,
    ) -> std::result::Result<Response<Vec<u8>>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        self.inner.send_http(request).await
    }
}

impl XrpcClient for AuthenticatedClient {
    fn base_uri(&self) -> String {
        self.inner.base_uri()
    }

    async fn authorization_token(&self, _is_refresh: bool) -> Option<AuthorizationToken> {
        Some(AuthorizationToken::Bearer(self.token.clone()))
    }
}

/// Client for interacting with PDS instances
pub struct PdsClient {
    http_client: reqwest::Client,
}

impl PdsClient {
    pub fn new(http_client: reqwest::Client) -> Self {
        Self { http_client }
    }

    /// Decode a JWT token without verification to extract claims
    /// The PDS will verify the token when we use it for upload
    fn decode_token_claims(token: &str) -> Result<ServiceAuthClaims> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return Err(Error::Unauthorized("Invalid JWT format".to_string()));
        }

        let payload = URL_SAFE_NO_PAD
            .decode(parts[1])
            .map_err(|e| Error::Unauthorized(format!("Invalid JWT payload encoding: {}", e)))?;

        let claims: ServiceAuthClaims = serde_json::from_slice(&payload)
            .map_err(|e| Error::Unauthorized(format!("Invalid JWT claims: {}", e)))?;

        Ok(claims)
    }

    /// Extract the PDS DID from a service auth token
    pub fn extract_pds_did(token: &str) -> Result<String> {
        let claims = Self::decode_token_claims(token)?;
        Ok(claims.aud)
    }

    /// Extract the user DID from a service auth token
    pub fn extract_user_did(token: &str) -> Result<String> {
        let claims = Self::decode_token_claims(token)?;
        // Use sub if present, otherwise use iss
        Ok(claims.sub.unwrap_or(claims.iss))
    }

    /// Resolve a DID to find the PDS endpoint
    pub async fn resolve_pds_endpoint(&self, did: &str) -> Result<String> {
        // For did:web, we can derive the endpoint directly from the domain
        // did:web:example.com -> https://example.com
        // This is the standard AT Protocol approach - the PDS endpoint is the domain itself
        if did.starts_with("did:web:") {
            let domain = did.strip_prefix("did:web:").unwrap();
            let endpoint = format!("https://{}", domain);

            // Optionally try to resolve the DID document for additional verification,
            // but fall back to the direct endpoint if it doesn't exist
            let url = format!("https://{}/.well-known/did.json", domain);
            debug!("Attempting to resolve did:web via: {}", url);

            match self.http_client.get(&url).send().await {
                Ok(response) if response.status().is_success() => {
                    if let Ok(doc) = response.json::<DidDocument>().await {
                        if let Ok(pds_endpoint) = self.extract_pds_from_did_doc(&doc, did) {
                            info!("Resolved {} via DID document to: {}", did, pds_endpoint);
                            return Ok(pds_endpoint);
                        }
                    }
                }
                _ => {
                    // DID document not found or invalid - use direct endpoint
                    debug!("No DID document found for {}, using direct endpoint: {}", did, endpoint);
                }
            }

            // Fall back to direct endpoint derivation
            info!("Using direct endpoint for {}: {}", did, endpoint);
            return Ok(endpoint);
        }

        // For did:plc, resolve via plc.directory
        if did.starts_with("did:plc:") {
            let url = format!("https://plc.directory/{}", did);
            debug!("Resolving did:plc via plc.directory: {}", url);

            let response = self.http_client.get(&url).send().await?;
            if !response.status().is_success() {
                return Err(Error::Internal(format!(
                    "Failed to resolve DID {}: {}",
                    did,
                    response.status()
                )));
            }

            let doc: DidDocument = response.json().await?;
            return self.extract_pds_from_did_doc(&doc, did);
        }

        Err(Error::Internal(format!("Unsupported DID method: {}", did)))
    }

    /// Extract PDS endpoint from a DID document
    fn extract_pds_from_did_doc(&self, doc: &DidDocument, did: &str) -> Result<String> {
        for service in &doc.service {
            if service.id.ends_with("#atproto_pds")
                && service.service_type == "AtprotoPersonalDataServer"
            {
                info!("Resolved {} to PDS: {}", did, service.service_endpoint);
                return Ok(service.service_endpoint.clone());
            }
        }

        Err(Error::Internal(format!(
            "Could not find PDS endpoint for DID: {}",
            did
        )))
    }

    /// Upload a blob to a PDS using the provided service auth token
    ///
    /// The token's `aud` claim must be the PDS DID. The PDS will validate
    /// the token signature before accepting the upload.
    ///
    /// # Arguments
    /// * `token` - Service auth token with PDS DID as audience
    /// * `data` - The blob data to upload
    /// * `mime_type` - MIME type of the blob
    ///
    /// # Returns
    /// The blob reference from the PDS (with valid CID)
    pub async fn upload_blob(
        &self,
        token: &str,
        data: Bytes,
        mime_type: &str,
    ) -> Result<BlobRef> {
        // Extract PDS DID from token
        let pds_did = Self::extract_pds_did(token)?;
        info!("Uploading blob to PDS: {}", pds_did);

        // Resolve PDS endpoint
        let pds_endpoint = self.resolve_pds_endpoint(&pds_did).await?;
        debug!("PDS endpoint: {}", pds_endpoint);

        // Create authenticated client
        let client = AuthenticatedClient::new(&pds_endpoint, token.to_string());
        let service = atrium_api::client::AtpServiceClient::new(client);

        // Upload blob
        let size = data.len();
        debug!("Uploading {} bytes ({}) to {}", size, mime_type, pds_endpoint);

        let output = service
            .service
            .com
            .atproto
            .repo
            .upload_blob(data.to_vec())
            .await
            .map_err(|e| Error::Internal(format!("PDS upload failed: {}", e)))?;

        info!("Blob uploaded to PDS: size={}", size);

        Ok(output.data.blob)
    }
}

/// Extract the CID string from a BlobRef
pub fn extract_cid(blob: &BlobRef) -> Option<String> {
    match blob {
        BlobRef::Typed(TypedBlobRef::Blob(b)) => Some(b.r#ref.0.to_string()),
        BlobRef::Untyped(u) => Some(u.cid.clone()),
    }
}

/// Convert atrium BlobRef to JSON value for storage
pub fn blob_ref_to_json(blob: &BlobRef) -> JsonValue {
    match blob {
        BlobRef::Typed(TypedBlobRef::Blob(b)) => {
            serde_json::json!({
                "$type": "blob",
                "ref": {
                    "$link": b.r#ref.0.to_string()
                },
                "mimeType": b.mime_type,
                "size": b.size
            })
        }
        BlobRef::Untyped(u) => {
            // Legacy format - shouldn't happen for new uploads
            serde_json::json!({
                "cid": u.cid,
                "mimeType": u.mime_type
            })
        }
    }
}
