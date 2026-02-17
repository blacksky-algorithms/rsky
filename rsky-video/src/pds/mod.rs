//! PDS (Personal Data Server) client for uploading blobs
//!
//! Handles uploading video blobs to users' PDS instances. The video service
//! creates its own service auth tokens (signed with its private key) to upload
//! blobs on behalf of users.

use atrium_api::types::{BlobRef, TypedBlobRef};
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
                    debug!(
                        "No DID document found for {}, using direct endpoint: {}",
                        did, endpoint
                    );
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

    /// Upload a blob to a PDS by forwarding the client's service auth token
    ///
    /// The client provides a service auth token from their PDS with:
    /// - iss: user's DID
    /// - aud: user's PDS DID
    /// - lxm: com.atproto.repo.uploadBlob
    ///
    /// We forward this token to the PDS, which verifies it against the user's DID document.
    ///
    /// # Arguments
    /// * `client_token` - The service auth token from the client
    /// * `user_did` - The user's DID
    /// * `data` - The blob data to upload
    /// * `mime_type` - MIME type of the blob
    ///
    /// # Returns
    /// The blob reference from the PDS (with valid CID)
    pub async fn upload_blob_with_token(
        &self,
        client_token: &str,
        user_did: &str,
        data: Bytes,
        mime_type: &str,
    ) -> Result<BlobRef> {
        // Resolve user's PDS endpoint from their DID
        let pds_endpoint = self.resolve_pds_endpoint(user_did).await?;

        info!("Uploading blob to PDS: {} using client token", pds_endpoint);
        let token = client_token;

        // Upload blob via direct HTTP request (not using atrium client)
        // atrium's client has issues with the auth header for this use case
        let upload_url = format!("{}/xrpc/com.atproto.repo.uploadBlob", pds_endpoint);
        let size = data.len();
        debug!("Uploading {} bytes to {}", size, upload_url);

        let response = self
            .http_client
            .post(&upload_url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", mime_type)
            .body(data.to_vec())
            .send()
            .await
            .map_err(|e| Error::Internal(format!("PDS upload request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::Internal(format!(
                "PDS upload failed: {} - {}",
                status, body
            )));
        }

        // Parse response
        #[derive(Deserialize)]
        struct UploadBlobResponse {
            blob: BlobRefResponse,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct BlobRefResponse {
            #[serde(rename = "$type")]
            blob_type: Option<String>,
            #[serde(rename = "ref")]
            cid_ref: CidRef,
            mime_type: String,
            size: u64,
        }

        #[derive(Deserialize)]
        struct CidRef {
            #[serde(rename = "$link")]
            link: String,
        }

        let upload_response: UploadBlobResponse = response
            .json()
            .await
            .map_err(|e| Error::Internal(format!("Failed to parse PDS response: {}", e)))?;

        info!(
            "Blob uploaded to PDS: size={}, cid={}",
            size, upload_response.blob.cid_ref.link
        );

        // Convert to atrium BlobRef format
        // We need to construct the proper BlobRef type
        let blob_ref = BlobRef::Typed(TypedBlobRef::Blob(atrium_api::types::Blob {
            r#ref: atrium_api::types::CidLink(
                cid::Cid::try_from(upload_response.blob.cid_ref.link.as_str())
                    .map_err(|e| Error::Internal(format!("Invalid CID from PDS: {}", e)))?,
            ),
            mime_type: upload_response.blob.mime_type,
            size: upload_response.blob.size as usize,
        }));

        Ok(blob_ref)
    }

    /// Convert an endpoint URL to a did:web
    fn endpoint_to_did(&self, endpoint: &str) -> Result<String> {
        let url = url::Url::parse(endpoint)
            .map_err(|e| Error::Internal(format!("Invalid endpoint URL: {}", e)))?;

        let host = url
            .host_str()
            .ok_or_else(|| Error::Internal("Endpoint has no host".to_string()))?;

        Ok(format!("did:web:{}", host))
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
