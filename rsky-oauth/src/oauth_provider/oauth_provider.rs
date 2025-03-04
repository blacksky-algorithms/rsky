use crate::oauth_provider::metadata::build_metadata::{build_metadata, CustomMetadata};
use crate::oauth_types::{OAuthAuthorizationServerMetadata, OAuthIssuerIdentifier};

pub struct OAuthProviderOptions {
    /**
     * Additional metadata to be included in the discovery document.
     */
    pub metadata: Option<CustomMetadata>,
    pub issuer: OAuthIssuerIdentifier,
}

pub struct OAuthProvider {
    pub metadata: OAuthAuthorizationServerMetadata,
}

impl OAuthProvider {
    pub fn new(options: OAuthProviderOptions) -> Self {
        let metadata = build_metadata(options.issuer, options.metadata);
        Self { metadata }
    }
}
