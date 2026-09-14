//! The routing decision for every request, and the axum service that
//! applies it.

use crate::classify::{self, Attribution, Kind, ReadClass};
use crate::inventory::Target;
use crate::journal::Journal;
use crate::lookup::AccountLookup;
use crate::metrics::METRICS;
use crate::policy::{AdmissionState, Backend, Routing};
use crate::proxy::{self, Body, Forwarder, ProxyError};
use axum::extract::State;
use axum::response::IntoResponse;
use bytes::Bytes;
use http::{HeaderMap, Method, Request, Response, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The most of a JSON mutation body the router reads to find its target.
pub const JSON_BODY_LIMIT: usize = 1024 * 1024;

pub struct Upstreams {
    pub ts_main: String,
    pub ts_sync: String,
    pub ts_read: Vec<String>,
    pub rsky: String,
    pub oauth: String,
}

pub struct Deadlines {
    pub read: Duration,
    pub sync: Duration,
    pub write: Duration,
}

pub struct Router {
    pub routing: Arc<Routing>,
    pub upstreams: Upstreams,
    pub deadlines: Deadlines,
    pub journal: Journal,
    pub lookup: AccountLookup,
    pub forwarder: Forwarder,
    round_robin: AtomicUsize,
}

impl Router {
    pub fn new(
        routing: Arc<Routing>,
        upstreams: Upstreams,
        deadlines: Deadlines,
        journal: Journal,
        lookup: AccountLookup,
    ) -> Self {
        Self {
            routing,
            upstreams,
            deadlines,
            journal,
            lookup,
            forwarder: Forwarder::new(),
            round_robin: AtomicUsize::new(0),
        }
    }

    /// The TypeScript pool a read class falls back to.
    fn ts_pool(&self, class: ReadClass) -> String {
        match class {
            ReadClass::Sync => self.upstreams.ts_sync.clone(),
            ReadClass::Bsky => {
                let n = self.round_robin.fetch_add(1, Ordering::Relaxed);
                self.upstreams.ts_read[n % self.upstreams.ts_read.len()].clone()
            }
            ReadClass::Main => self
                .upstreams
                .ts_read
                .first()
                .cloned()
                .unwrap_or_else(|| self.upstreams.ts_main.clone()),
        }
    }

    /// The DID an identity names, through the account tables when it is a
    /// handle or email; an unknown identity pins nothing. A lookup that
    /// cannot be made is an error the caller decides on: a read falls back
    /// to the default backend, a mutation is refused.
    fn did_of(&self, identity: &str) -> rusqlite::Result<Option<String>> {
        self.lookup.did_for_identifier(identity)
    }

    /// Every DID a mutation names, resolved; `Err` carries the refusal.
    fn targets(&self, attribution: Attribution) -> Result<Vec<String>, Response<Body>> {
        let policy = self.routing.policy();
        match attribution {
            Attribution::None => Ok(Vec::new()),
            Attribution::Identifier(identifier) => Ok(self
                .did_of(&identifier)
                .map_err(|err| lookup_unavailable("account lookup failed", &err))?
                .into_iter()
                .collect()),
            Attribution::Identifiers(identifiers) => {
                let mut dids = Vec::new();
                for identifier in identifiers {
                    dids.extend(
                        self.did_of(&identifier)
                            .map_err(|err| lookup_unavailable("account lookup failed", &err))?,
                    );
                }
                Ok(dids)
            }
            Attribution::EmailToken(token) => Ok(self
                .lookup
                .did_for_email_token(&token)
                .map_err(|err| lookup_unavailable("token lookup failed", &err))?
                .into_iter()
                .collect()),
            Attribution::Malformed(reason) => Err(refused(
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                reason,
                "malformed",
            )),
            Attribution::Unattributable(reason) => {
                if policy.has_canaries() {
                    Err(refused(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "RouterUnattributable",
                        reason,
                        "unattributable",
                    ))
                } else {
                    Ok(Vec::new())
                }
            }
        }
    }

    /// Where a mutation for `dids` goes, or the refusal.
    fn write_backend(&self, dids: &[String]) -> Result<(Backend, &'static str), Response<Body>> {
        let policy = self.routing.policy();
        let allowlist = self.routing.allowlist();
        let mut backend = None;
        for did in dids {
            if policy.canary_fence.contains(did) {
                return Err(refused(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "RouterFenced",
                    "the account is fenced",
                    "fenced",
                ));
            }
            if policy.canary_rsky.contains(did) {
                if policy.kill_switch {
                    return Err(refused(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "RouterKillSwitch",
                        "the account's writer is paused",
                        "kill-switch",
                    ));
                }
                if allowlist.state_of(did) != AdmissionState::Active {
                    return Err(refused(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "RouterNotAdmitted",
                        "the account's writer is not admitting writes",
                        "not-admitted",
                    ));
                }
                match backend {
                    None | Some(Backend::Rsky) => backend = Some(Backend::Rsky),
                    Some(Backend::Ts) => {
                        return Err(refused(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "RouterSplitTargets",
                            "the request names accounts on different writers",
                            "split-targets",
                        ))
                    }
                }
            } else {
                match backend {
                    None | Some(Backend::Ts) => backend = Some(Backend::Ts),
                    Some(Backend::Rsky) => {
                        return Err(refused(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "RouterSplitTargets",
                            "the request names accounts on different writers",
                            "split-targets",
                        ))
                    }
                }
            }
        }
        Ok(match backend {
            Some(Backend::Rsky) => (Backend::Rsky, "canary"),
            Some(Backend::Ts) => (Backend::Ts, "target"),
            None => (policy.writes_default, "default"),
        })
    }

    fn write_upstream(&self, backend: Backend, nsid: &str) -> String {
        match backend {
            Backend::Rsky => self.upstreams.rsky.clone(),
            Backend::Ts
                if nsid == "com.atproto.repo.uploadBlob"
                    || nsid == "com.atproto.repo.importRepo" =>
            {
                self.upstreams.ts_sync.clone()
            }
            Backend::Ts => self.upstreams.ts_main.clone(),
        }
    }

    fn forward_read<'a>(
        &'a self,
        class: ReadClass,
        identity: Option<String>,
        method: Method,
        path_and_query: &'a str,
        headers: &'a HeaderMap,
    ) -> Pin<Box<dyn Future<Output = Response<Body>> + Send + 'a>> {
        Box::pin(self.forward_read_inner(class, identity, method, path_and_query, headers))
    }

    async fn forward_read_inner(
        &self,
        class: ReadClass,
        identity: Option<String>,
        method: Method,
        path_and_query: &str,
        headers: &HeaderMap,
    ) -> Response<Body> {
        let did = identity.as_deref().and_then(|identity| {
            self.did_of(identity).unwrap_or_else(|err| {
                tracing::warn!(%err, "account lookup failed; the read takes the default backend");
                None
            })
        });
        let backend = self.routing.policy().read_backend(did.as_deref());
        let deadline = match class {
            ReadClass::Sync => self.deadlines.sync,
            _ => self.deadlines.read,
        };
        let class_name = class_label(class);
        let started = Instant::now();
        let (upstream, backend_name, reason) = match backend {
            Backend::Rsky => (self.upstreams.rsky.clone(), "rsky", "pinned"),
            Backend::Ts => (self.ts_pool(class), "ts", "default"),
        };
        let first = self
            .forwarder
            .forward(
                &upstream,
                method.clone(),
                path_and_query,
                headers,
                proxy::empty_body(),
                deadline,
            )
            .await;
        let (result, backend_name, reason) = match first {
            Err(ProxyError::Unreachable(upstream, err)) => {
                tracing::warn!(%upstream, %err, "read upstream unreachable; failing over");
                METRICS.failovers.with_label_values(&[class_name]).inc();
                let fallback = self.ts_pool(class);
                (
                    self.forwarder
                        .forward(
                            &fallback,
                            method,
                            path_and_query,
                            headers,
                            proxy::empty_body(),
                            deadline,
                        )
                        .await,
                    "ts",
                    "failover",
                )
            }
            other => (other, backend_name, reason),
        };
        self.finish(result, backend_name, reason, class_name, started)
    }

    fn finish(
        &self,
        result: Result<Response<Body>, ProxyError>,
        backend: &str,
        reason: &str,
        class: &str,
        started: Instant,
    ) -> Response<Body> {
        let mut response = match result {
            Ok(response) => response,
            Err(ProxyError::Timeout(upstream)) => {
                tracing::warn!(%upstream, "upstream timed out");
                proxy::json_error(
                    StatusCode::GATEWAY_TIMEOUT,
                    "UpstreamTimeout",
                    "the upstream did not answer in time",
                )
            }
            Err(err) => {
                tracing::warn!(%err, "upstream failed");
                let mut response = proxy::json_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "UpstreamUnavailable",
                    "the upstream could not be reached",
                );
                response.headers_mut().insert(
                    http::header::RETRY_AFTER,
                    http::HeaderValue::from_static("1"),
                );
                response
            }
        };
        proxy::stamp(&mut response, backend, reason);
        METRICS
            .requests
            .with_label_values(&[backend, class, response.status().as_str()])
            .inc();
        METRICS
            .latency
            .with_label_values(&[backend, class])
            .observe(started.elapsed().as_secs_f64());
        response
    }

    fn forward_mutation<'a>(
        &'a self,
        nsid: &'a str,
        target: Option<Target>,
        method: Method,
        path_and_query: &'a str,
        headers: &'a HeaderMap,
        body: Body,
    ) -> Pin<Box<dyn Future<Output = Response<Body>> + Send + 'a>> {
        Box::pin(self.forward_mutation_inner(nsid, target, method, path_and_query, headers, body))
    }

    async fn forward_mutation_inner(
        &self,
        nsid: &str,
        target: Option<Target>,
        method: Method,
        path_and_query: &str,
        headers: &HeaderMap,
        body: Body,
    ) -> Response<Body> {
        let started = Instant::now();
        // A procedure outside the pinned lexicon set is not a repository
        // mutation: the PDS's catchall proxies it to the service it names
        // (chat, appview, moderation), so it follows the default writer and
        // is journaled like any other. The OAuth UI API is a closed set, so
        // an endpoint missing from it is still refused.
        let (target, proxied) = match target {
            Some(target) => (target, false),
            None if nsid.starts_with("~api/") => {
                METRICS
                    .writes_rejected
                    .with_label_values(&["unknown"])
                    .inc();
                return refused(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "RouterUnknownMutation",
                    "this mutation is not in the router's inventory",
                    "unknown",
                );
            }
            None => (Target::None, true),
        };
        // targets read from the body need it buffered; raw bodies never are
        let needs_body = !matches!(target, Target::AuthSubject | Target::None);
        let (body, parsed) = if needs_body {
            match buffer_json(headers, body).await {
                Ok((bytes, parsed)) => (proxy::full_body(bytes), parsed),
                Err(response) => return response,
            }
        } else {
            (body, None)
        };
        let attribution = classify::attribute(target, parsed.as_ref(), headers);
        let dids = match self.targets(attribution) {
            Ok(dids) => dids,
            Err(response) => {
                METRICS
                    .writes_rejected
                    .with_label_values(&["malformed"])
                    .inc();
                return response;
            }
        };
        let (backend, reason) = match self.write_backend(&dids) {
            Ok((backend, _)) if proxied => (backend, "proxied"),
            Ok(decision) => decision,
            Err(response) => {
                let reason = response
                    .headers()
                    .get("x-router-reason")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("refused")
                    .to_owned();
                METRICS.writes_rejected.with_label_values(&[&reason]).inc();
                return response;
            }
        };
        if backend == Backend::Rsky && nsid.starts_with("~api/") {
            METRICS
                .writes_rejected
                .with_label_values(&["no-equivalent"])
                .inc();
            return refused(
                StatusCode::SERVICE_UNAVAILABLE,
                "RouterNoEquivalent",
                "the account's writer does not serve this endpoint",
                "no-equivalent",
            );
        }
        let upstream = self.write_upstream(backend, nsid);
        let journaled =
            self.journal
                .start(nsid, dids.first().map(String::as_str), backend.as_str());
        let id = match journaled {
            Ok(id) => id,
            Err(err) => {
                tracing::error!(%err, "journal write failed; refusing the mutation");
                METRICS.journal_failures.inc();
                return refused(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "RouterJournalUnavailable",
                    "the mutation journal could not be written",
                    "journal",
                );
            }
        };
        let result = self
            .forwarder
            .forward(
                &upstream,
                method,
                path_and_query,
                headers,
                body,
                self.deadlines.write,
            )
            .await;
        // a request that was sent and never answered may still complete
        // upstream; the journal says so instead of claiming an outcome
        let outcome_unknown = matches!(
            result,
            Err(ProxyError::Timeout(_)) | Err(ProxyError::Upstream(..))
        );
        let response = self.finish(result, backend.as_str(), reason, "write", started);
        if backend == Backend::Ts {
            let now = self.routing.policy();
            if dids
                .iter()
                .any(|did| now.canary_rsky.contains(did) || now.canary_fence.contains(did))
            {
                tracing::error!(nsid, ?dids, "policy changed while a mutation was in flight");
                METRICS.misroutes.inc();
            }
        }
        let closed = if outcome_unknown {
            self.journal.ambiguous(id, response.status().as_u16())
        } else {
            self.journal.end(id, response.status().as_u16())
        };
        if let Err(err) = closed {
            tracing::error!(%err, "journal end line failed");
            METRICS.journal_failures.inc();
        }
        response
    }

    /// Answers one request; boxed over the owned handle so the future's
    /// borrows have one concrete lifetime.
    pub fn handle(
        self: Arc<Self>,
        request: Request<axum::body::Body>,
    ) -> Pin<Box<dyn Future<Output = Response<Body>> + Send>> {
        Box::pin(async move { self.answer(request).await })
    }

    async fn answer(&self, request: Request<axum::body::Body>) -> Response<Body> {
        let (parts, body) = request.into_parts();
        let path = parts.uri.path().to_owned();
        let query = parts.uri.query().map(str::to_owned);
        let path_and_query = parts
            .uri
            .path_and_query()
            .map(|pq| pq.as_str().to_owned())
            .unwrap_or_else(|| path.clone());
        let body: Body = body.map_err(proxy::BoxError::from).boxed_unsync();
        match classify::classify(&parts.method, &path, query.as_deref(), &parts.headers) {
            Kind::RouterHealth => {
                let mut response = proxy::json_error(StatusCode::OK, "ok", "router");
                proxy::stamp(&mut response, "router", "health");
                response
            }
            Kind::OauthAs => {
                let started = Instant::now();
                let result = self
                    .forwarder
                    .forward(
                        &self.upstreams.oauth,
                        parts.method,
                        &path_and_query,
                        &parts.headers,
                        body,
                        self.deadlines.write,
                    )
                    .await;
                self.finish(result, "oauth", "authorization-server", "oauth", started)
            }
            Kind::Read { class, identity } => {
                self.forward_read(
                    class,
                    identity,
                    parts.method,
                    &path_and_query,
                    &parts.headers,
                )
                .await
            }
            Kind::Procedure { nsid, target } => {
                self.forward_mutation(
                    &nsid,
                    target,
                    parts.method,
                    &path_and_query,
                    &parts.headers,
                    body,
                )
                .await
            }
            Kind::Api { endpoint, target } => {
                self.forward_mutation(
                    &format!("~api/{endpoint}"),
                    target,
                    parts.method,
                    &path_and_query,
                    &parts.headers,
                    body,
                )
                .await
            }
            Kind::UnknownMutation => {
                METRICS
                    .writes_rejected
                    .with_label_values(&["unknown"])
                    .inc();
                refused(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "RouterUnknownMutation",
                    "this mutation is not in the router's inventory",
                    "unknown",
                )
            }
        }
    }
}

fn class_label(class: ReadClass) -> &'static str {
    match class {
        ReadClass::Sync => "sync",
        ReadClass::Bsky => "bsky",
        ReadClass::Main => "main",
    }
}

/// A mutation whose target could not be resolved is refused, never routed
/// as if it named no account.
fn lookup_unavailable(what: &str, err: &rusqlite::Error) -> Response<Body> {
    tracing::error!(%err, "{what}; refusing the mutation");
    METRICS.writes_rejected.with_label_values(&["lookup"]).inc();
    refused(
        StatusCode::SERVICE_UNAVAILABLE,
        "RouterLookupUnavailable",
        "the account lookup is unavailable",
        "lookup",
    )
}

fn refused(status: StatusCode, error: &str, message: &str, reason: &str) -> Response<Body> {
    let mut response = proxy::json_error(status, error, message);
    if status == StatusCode::SERVICE_UNAVAILABLE {
        response.headers_mut().insert(
            http::header::RETRY_AFTER,
            http::HeaderValue::from_static("1"),
        );
    }
    proxy::stamp(&mut response, "router", reason);
    response
}

/// Buffers a JSON body up to the limit and parses it; a body that is not
/// JSON is forwarded unparsed.
async fn buffer_json(
    headers: &HeaderMap,
    body: Body,
) -> Result<(Bytes, Option<Value>), Response<Body>> {
    let is_json = headers
        .get(http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.starts_with("application/json"))
        .unwrap_or(false);
    let mut body = body;
    let mut buffer = Vec::new();
    while let Some(frame) = body.frame().await {
        let frame = match frame {
            Ok(frame) => frame,
            Err(err) => {
                tracing::warn!(%err, "mutation body failed");
                return Err(refused(
                    StatusCode::BAD_REQUEST,
                    "InvalidRequest",
                    "the mutation body could not be read",
                    "body",
                ));
            }
        };
        if let Ok(data) = frame.into_data() {
            if buffer.len() + data.len() > JSON_BODY_LIMIT {
                return Err(refused(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "PayloadTooLarge",
                    "the mutation body exceeds the router's limit",
                    "too-large",
                ));
            }
            buffer.extend_from_slice(&data);
        }
    }
    let bytes = Bytes::from(buffer);
    let parsed = if is_json {
        serde_json::from_slice::<Value>(&bytes).ok()
    } else {
        None
    };
    Ok((bytes, parsed))
}

/// The axum handler for every path.
pub async fn serve(
    State(router): State<Arc<Router>>,
    request: Request<axum::body::Body>,
) -> impl IntoResponse {
    let response = router.handle(request).await;
    let (parts, body) = response.into_parts();
    Response::from_parts(parts, axum::body::Body::new(body))
}

pub fn app(router: Arc<Router>) -> axum::Router {
    axum::Router::new().fallback(serve).with_state(router)
}

/// The metrics service on its own port.
pub fn metrics_app() -> axum::Router {
    axum::Router::new().route(
        "/metrics",
        axum::routing::get(|| async { METRICS.render() }),
    )
}
