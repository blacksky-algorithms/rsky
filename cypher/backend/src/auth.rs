use crate::vendored::atrium_oauth_client::{
    AtprotoLocalhostClientMetadata, DefaultHttpClient, KnownScope, OAuthClient, OAuthClientConfig,
    OAuthResolverConfig, Scope,
};
use atrium_identity::did::{CommonDidResolver, CommonDidResolverConfig, DEFAULT_PLC_DIRECTORY_URL};
use atrium_identity::handle::{AtprotoHandleResolver, AtprotoHandleResolverConfig, DnsTxtResolver};
use hickory_resolver::TokioAsyncResolver;
use std::sync::Arc;

pub struct HickoryDnsTxtResolver {
    resolver: TokioAsyncResolver,
}

impl Default for HickoryDnsTxtResolver {
    fn default() -> Self {
        Self {
            resolver: TokioAsyncResolver::tokio_from_system_conf()
                .expect("failed to create resolver"),
        }
    }
}

impl DnsTxtResolver for HickoryDnsTxtResolver {
    async fn resolve(
        &self,
        query: &str,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        Ok(self
            .resolver
            .txt_lookup(query)
            .await?
            .iter()
            .map(|txt| txt.to_string())
            .collect())
    }
}

pub fn init_oauth() -> OAuthClient<
    crate::vendored::atrium_oauth_client::store::state::MemoryStateStore,
    CommonDidResolver<DefaultHttpClient>,
    AtprotoHandleResolver<HickoryDnsTxtResolver, DefaultHttpClient>,
> {
    // Prepare HTTP client and resolvers required by Atrium
    let http_client = Arc::new(DefaultHttpClient::default());
    let resolver_config = OAuthResolverConfig {
        did_resolver: CommonDidResolver::new(CommonDidResolverConfig {
            plc_directory_url: DEFAULT_PLC_DIRECTORY_URL.to_string(),
            http_client: http_client.clone(),
        }),
        handle_resolver: AtprotoHandleResolver::new(AtprotoHandleResolverConfig {
            dns_txt_resolver: /* a DNS resolver, e.g., use system config */
            HickoryDnsTxtResolver::default(),
            http_client: http_client.clone(),
        }),
        authorization_server_metadata: Default::default(),
        protected_resource_metadata: Default::default(),
    };
    let client_config = OAuthClientConfig {
        client_metadata: AtprotoLocalhostClientMetadata {
            // Our redirect URI (use the same port as the Axum server)
            redirect_uris: Some(vec![String::from("http://127.0.0.1:8080/callback")]),
            scopes: Some(vec![
                Scope::Known(KnownScope::Atproto),           // basic Bluesky access
                Scope::Known(KnownScope::TransitionGeneric), // (likely refresh/offline scope)
            ]),
        },
        keys: None,
        resolver: resolver_config,
        state_store: crate::vendored::atrium_oauth_client::store::state::MemoryStateStore::default(
        ),
    };
    OAuthClient::new(client_config).expect("Failed to create OAuthClient")
}
