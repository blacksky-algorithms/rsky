use std::num::NonZeroUsize;
use std::time::{Duration, SystemTime};

use futures_util::TryStreamExt;
use jacquard_common::deps::smol_str::SmolStr;
use jacquard_common::error::{ClientError, ClientErrorKind, HttpError};
use jacquard_common::stream::ByteStream;
use jacquard_common::xrpc::{RespOutput, XrpcError as JXE, XrpcExt, XrpcRequest, XrpcResp};
use jacquard_common::{AuthorizationToken, StreamError, StreamErrorKind};
use tokio::io::AsyncRead;
use tokio_util::io::StreamReader;
use tracing::{debug, info, trace, warn};

use super::{Host, HostClientError, InBackoff, MAX_ERROR_BODY, parse_retry_after};
use crate::sync_handle::{GetRepoStatusResponse, ListReposResponse}; // bleh
use crate::{Did, SensitiveToken};

#[derive(Debug, thiserror::Error)]
pub enum HostRequestError {
    #[error("failed to construct base URL: {0}")]
    BadBase(String),
    #[error("host backing off, {remaining:?} remaining")]
    HostBackoff { remaining: Duration },
    #[error("rate limited. retry: {retry_after:?}")]
    RateLimited { retry_after: Option<SystemTime> },
    #[error("server transient")]
    ServerTransient,
    #[error("bad request")]
    BadRequest,
    #[error("other client error: {0:?}")]
    OtherClient(http::StatusCode),
    #[error("other server error: {0:?}")]
    OtherServer(http::StatusCode),
    #[error("other http error: {0:?}")]
    OtherHttp(http::StatusCode),
    #[error("xrpc error: {status}: {error:?}")]
    Xrpc {
        status: http::StatusCode,
        error: XrpcError,
    },
    #[error("failed to decode response: {0}")]
    Decode(String),
    #[error("transport error: {0}")]
    Transport(String),
    #[error("request timed out")]
    Timeout,
    #[error("request cancelled")]
    Cancelled,
    #[error("registry gone")]
    RegistryGone,
}

impl HostRequestError {
    fn from_details(
        status: http::StatusCode,
        headers: Option<&http::HeaderMap>,
        detail: Option<XrpcError>,
    ) -> Self {
        use http::StatusCode as S;
        match status {
            S::TOO_MANY_REQUESTS => {
                let retry_after = headers.and_then(|h| parse_retry_after(h, SystemTime::now()));
                HostRequestError::RateLimited { retry_after }
            }
            S::BAD_GATEWAY | S::SERVICE_UNAVAILABLE | S::GATEWAY_TIMEOUT => {
                HostRequestError::ServerTransient
            }
            s if s.is_client_error() => detail
                .map(|error| Self::Xrpc { status: s, error })
                .unwrap_or_else(|| {
                    if s == S::BAD_REQUEST {
                        Self::BadRequest
                    } else {
                        Self::OtherClient(s)
                    }
                }),
            s if s.is_server_error() => detail
                .map(|error| Self::Xrpc { status: s, error })
                .unwrap_or(Self::OtherServer(s)),
            s => HostRequestError::OtherHttp(s),
        }
    }
}

impl From<&HostClientError> for HostRequestError {
    fn from(hce: &HostClientError) -> Self {
        use HostClientError as E;
        match hce {
            E::Cancelled => Self::Cancelled,
            E::Timeout => Self::Timeout,
            E::Backoff(InBackoff { remaining }) => Self::HostBackoff {
                remaining: *remaining,
            },
            E::RegistryGone => Self::RegistryGone,
            E::BadRedirect(e) => Self::Transport(format!("bad redirect: {e}")),
            E::BodyTooBig => Self::Transport("body too big".to_string()),
            E::Transport(e) => Self::Transport(format!("reqwest: {e}")),
        }
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct XrpcError {
    pub error: String,
    pub message: Option<String>,
}

impl Host {
    #[tracing::instrument(
        skip_all,
        fields(
            host = self.name.as_str(),
            method = R::METHOD.as_str(),
            nsid = R::NSID,
        ),
    )]
    pub async fn send<R>(
        &self,
        req: &R,
    ) -> Result<RespOutput<SmolStr, R::Response>, HostRequestError>
    where
        R: XrpcRequest + Send + Sync + serde::Serialize,
        R::Response: Send + Sync,
        RespOutput<SmolStr, R::Response>: serde::de::DeserializeOwned,
    {
        let base = self
            .jacquard_uri_base("https")
            .map_err(HostRequestError::BadBase)?;

        let res = self
            .client
            .as_ref()
            .xrpc(base.borrow())
            .send(req)
            .await
            .map_err(map_client_error)?;

        let status = res.status();

        if !status.is_success() {
            // non-ok status: map out some edge cases from jacquard (TODO: probably open an issue)
            let detail = match res.into_output() {
                Ok(_) => unreachable!("into_output() cannot be Ok(_) if is_success = false"),
                Err(JXE::Generic(g)) => Some(XrpcError {
                    error: g.error.to_string(),
                    message: g.message.map(Into::into),
                }),
                Err(JXE::Xrpc(_)) => unreachable!("old jacquard variant?"),
                Err(JXE::Auth(err)) => {
                    // TODO: probably surface auth errors better
                    debug!(?err, %status, "auth error");
                    None
                }
                Err(err) => {
                    debug!(?err, %status, "other error, probably undecodable non-2xx");
                    None
                }
            };
            return Err(HostRequestError::from_details(status, None, detail));
        }

        res.into_output().map_err(|err| {
            // is_succes=true: only possible error is decode error
            debug!(%err, "failed to decode response");
            HostRequestError::Decode(err.to_string())
        })
    }

    pub fn download<R>(
        &self,
        req: R,
    ) -> impl Future<
        Output = Result<
            (impl AsyncRead + Send + Unpin + use<R>, http::Extensions),
            HostRequestError,
        >,
    >
    where
        R: XrpcRequest + Send + Sync + serde::Serialize,
        R::Response: XrpcResp + Send + Sync,
    {
        self.download_with_auth(req, None)
    }

    #[tracing::instrument(
        skip_all,
        fields(
            host = self.name.as_str(),
            method = R::METHOD.as_str(),
            nsid = R::NSID,
        ),
    )]
    async fn download_with_auth<R>(
        &self,
        req: R,
        auth: Option<AuthorizationToken<SmolStr>>,
    ) -> Result<(impl AsyncRead + Send + Unpin + use<R>, http::Extensions), HostRequestError>
    where
        R: XrpcRequest + Send + Sync + serde::Serialize,
        R::Response: XrpcResp + Send + Sync,
    {
        let base = self
            .jacquard_uri_base("https")
            .map_err(HostRequestError::BadBase)?;

        let mut call = self.client.as_ref().xrpc(base.borrow());
        if let Some(token) = auth {
            call = call.auth(token);
        }

        let (parts, body) = call
            .download(&req)
            .await
            .map_err(map_stream_error)?
            .into_parts();

        if !parts.status.is_success() {
            let decoded = read_xrpc_error(body, parts.status).await;
            // note: permit dropped here  ^^^^ after consuming body
            return Err(HostRequestError::from_details(
                parts.status,
                Some(&parts.headers),
                decoded,
            ));
        }

        let body = body.into_inner().map_err(std::io::Error::other);

        Ok((StreamReader::new(body), parts.extensions))
    }

    /// streaming com.atproto.sync.getRepo convenience wrapper
    pub fn get_repo(
        &self,
        did: &Did,
        token: Option<&SensitiveToken>,
    ) -> impl Future<Output = Result<(impl AsyncRead + Send + Unpin, http::Extensions), HostRequestError>>
    {
        use jacquard_api::com_atproto::sync::get_repo::GetRepo;
        let auth = token.map(|t| AuthorizationToken::Bearer(t.as_str().into()));
        self.download_with_auth(GetRepo::new().did(did).build(), auth)
    }

    /// com.atproto.sync.listRepos convenience wrapper
    pub async fn list_repos(
        &self,
        limit: NonZeroUsize,
        cursor: Option<&str>,
    ) -> Result<ListReposResponse, HostRequestError> {
        use jacquard_api::com_atproto::sync::list_repos::{ListRepos, RepoStatus};

        let req = ListRepos::new()
            .limit(usize::from(limit) as i64)
            .cursor(cursor.map(SmolStr::from))
            .build();

        let res = self.send(&req).await?;

        let repos = res
            .repos
            .into_iter()
            .map(|r| GetRepoStatusResponse {
                did: (&r.did).into(),
                rev: r.rev.into(),
                active: r.active.unwrap_or(false), // wat
                status: r.status.map(|s| match s {
                    RepoStatus::Takendown => "takendown",
                    RepoStatus::Suspended => "suspended",
                    RepoStatus::Deleted => "deleted",
                    RepoStatus::Deactivated => "deactivated",
                    RepoStatus::Desynchronized => "desynchronized",
                    RepoStatus::Throttled => "throttled",
                    RepoStatus::Other(s) => {
                        trace!(unknown_status = %s, "mapping unknown status to 'other'");
                        "other"
                    }
                }),
            })
            .collect();

        let cursor = res.cursor.map(String::from);

        Ok(ListReposResponse { repos, cursor })
    }

    /// com.atproto.sync.listHosts convenience wrapper
    pub async fn list_hosts(
        &self,
        limit: NonZeroUsize,
        cursor: Option<&str>,
    ) -> Result<ListHostsResponse, HostRequestError> {
        use jacquard_api::com_atproto::sync::HostStatus;
        use jacquard_api::com_atproto::sync::list_hosts::ListHosts;

        let req = ListHosts::new()
            .limit(usize::from(limit) as i64)
            .cursor(cursor.map(SmolStr::from))
            .build();

        let res = self.send(&req).await?;

        let hosts = res
            .hosts
            .into_iter()
            .map(|h| PdsHost {
                hostname: h.hostname.to_string(),
                account_count: h.account_count.unwrap_or(0).max(0) as u64,
                status: h.status.map(|s| match s {
                    HostStatus::Active => "active",
                    HostStatus::Idle => "idle",
                    HostStatus::Offline => "offline",
                    HostStatus::Throttled => "throttled",
                    HostStatus::Banned => "banned",
                    HostStatus::Other(_) => "other",
                }),
                seq: h.seq.unwrap_or(0),
            })
            .collect();

        Ok(ListHostsResponse {
            hosts,
            cursor: res.cursor.map(String::from),
        })
    }
}

pub struct ListHostsResponse {
    pub hosts: Vec<PdsHost>,
    pub cursor: Option<String>,
}
pub struct PdsHost {
    pub hostname: String,
    pub account_count: u64,
    pub status: Option<&'static str>,
    pub seq: i64,
}

// for later
// impl Host {
//     async fn get_record(&self, did: &Did, collection: &Nsid, rkey: &str) -> Result<GetRecord, XrpcError>;
//     async fn describe_repo(&self, did: &Did) -> Result<DescribeRepo, XrpcError>;
//     async fn get_repo_status(&self, did: &Did) -> Result<RepoStatus, XrpcError>;
// }

fn decode_xrpc_error(buf: &[u8], status: http::StatusCode) -> Option<XrpcError> {
    if buf.is_empty() {
        return None;
    }
    if buf.len() > MAX_ERROR_BODY {
        let s = String::from_utf8_lossy(&buf[..MAX_ERROR_BODY]);
        debug!(%status, %s, "error body too long, using status code only");
        return None;
    }
    serde_json::from_slice(buf)
        .inspect_err(|err| {
            let s = String::from_utf8_lossy(buf);
            debug!(%status, %s, ?err, "non-xrpc-looking error response body");
        })
        .ok()
}

async fn read_xrpc_error(body: ByteStream, status: http::StatusCode) -> Option<XrpcError> {
    use futures_util::StreamExt;
    let mut buf = Vec::new();
    let mut stream = body.into_inner();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(b) => {
                buf.extend_from_slice(&b);
                if buf.len() > MAX_ERROR_BODY {
                    let s = String::from_utf8_lossy(&buf);
                    info!(%s, "bad-request error response too long to use");
                    return None;
                }
            }
            Err(err) => {
                let s = String::from_utf8_lossy(&buf);
                info!(%s, ?err, "error reading bad-request response body");
                return None;
            }
        }
    }
    serde_json::from_slice(&buf)
        .inspect_err(|err| {
            let s = String::from_utf8_lossy(&buf);
            debug!(%status, %s, ?err, "non-xrpc-looking error response body");
        })
        .ok()
}

fn map_client_error(e: ClientError) -> HostRequestError {
    match e.kind() {
        ClientErrorKind::Http { status } => {
            // body is on the HttpError source. NO headers here (jacquard drops them on
            // the send path) ⇒ retry_after can't be read. TODO: needs jacquard to surface
            // response headers for send(); download() below *does* get them.
            let detail = e
                .source_err()
                .and_then(|s| s.downcast_ref::<HttpError>())
                .and_then(|h| h.body.as_deref())
                .and_then(|b| decode_xrpc_error(b, *status));
            HostRequestError::from_details(*status, None, detail)
        }
        ClientErrorKind::Transport
            if let Some(s) = e.source_err()
                && let Some(hce) = s.downcast_ref::<HostClientError>() =>
        {
            hce.into()
        }
        ClientErrorKind::Decode(err) => HostRequestError::Decode(err.to_string()),
        ClientErrorKind::Auth(a) => {
            debug!(?a, "auth challenge on an unauthenticated request");
            HostRequestError::OtherClient(http::StatusCode::UNAUTHORIZED) // FLAG: dedicated Auth variant?
        }
        ClientErrorKind::Encode(err) => {
            debug!(?err, "jacquard client request encode error");
            HostRequestError::Transport(e.to_string())
        }
        ClientErrorKind::InvalidRequest(err) => {
            debug!(?err, "jacquard client invalid request?");
            HostRequestError::Transport(e.to_string())
        }
        ClientErrorKind::IdentityResolution | ClientErrorKind::Storage => {
            unreachable!("we don't use jacquard identity resolution or session storage here");
        }
        err => {
            warn!(?err, "unhandled jacquard client error");
            HostRequestError::Transport(e.to_string())
        }
    }
}

/// extract our jacquard-transport-wrapped inner error, if present
fn map_stream_error(e: StreamError) -> HostRequestError {
    if matches!(e.kind(), StreamErrorKind::Transport)
        && let Some(s) = e.source()
        && let Some(hce) = s.downcast_ref::<HostClientError>()
    {
        return hce.into();
    }
    HostRequestError::Transport(e.to_string())
}
