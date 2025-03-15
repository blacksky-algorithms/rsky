use crate::vendored::atrium_oauth_client::resolver::Resolver;
use crate::vendored::atrium_oauth_client::types::OAuthAuthorizationServerMetadata;
use atrium_identity::{Error, Result};
use atrium_xrpc::HttpClient;
use atrium_xrpc::http::uri::Builder;
use atrium_xrpc::http::{Request, StatusCode, Uri};
use std::sync::Arc;

pub struct DefaultOAuthAuthorizationServerResolver<T> {
    http_client: Arc<T>,
}

impl<T> DefaultOAuthAuthorizationServerResolver<T> {
    pub fn new(http_client: Arc<T>) -> Self {
        Self { http_client }
    }
}

impl<T> Resolver for DefaultOAuthAuthorizationServerResolver<T>
where
    T: HttpClient + Send + Sync + 'static,
{
    type Input = String;
    type Output = OAuthAuthorizationServerMetadata;
    type Error = Error;

    async fn resolve(&self, issuer: &Self::Input) -> Result<Self::Output> {
        let uri = Builder::from(issuer.parse::<Uri>()?)
            .path_and_query("/.well-known/oauth-authorization-server")
            .build()?;
        let res = self
            .http_client
            .send_http(Request::builder().uri(uri).body(Vec::new())?)
            .await
            .map_err(Error::HttpClient)?;
        // https://datatracker.ietf.org/doc/html/rfc8414#section-3.2
        if res.status() == StatusCode::OK {
            let metadata = serde_json::from_slice::<OAuthAuthorizationServerMetadata>(res.body())?;
            // https://datatracker.ietf.org/doc/html/rfc8414#section-3.3
            if &metadata.issuer == issuer {
                Ok(metadata)
            } else {
                Err(Error::AuthorizationServerMetadata(format!(
                    "invalid issuer: {}",
                    metadata.issuer
                )))
            }
        } else {
            Err(Error::HttpStatus(res.status()))
        }
    }
}
