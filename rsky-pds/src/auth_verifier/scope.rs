//! The scope-declaration seam.
//!
//! A route cannot learn who is calling it without first naming what that
//! caller must be permitted to do. [`Scoped`] is the only type in the crate
//! that hands out an authenticated requester's DID, and it is generic over a
//! [`ScopeDecl`] -- so the declaration is written in the route's signature or
//! the route does not compile.
//!
//! This is the Rust analog of the reference PDS's `authorize` field, which is
//! required (not optional) on every OAuth-capable auth verifier's options,
//! combined with tranquil-pds's trick of making the authenticated subject
//! reachable only through a scope proof.
//!
//! Two declarations mean "nothing is required here", mirroring the reference
//! implementation's two opt-out spellings, and both are one grep away:
//!
//! ```text
//! rg 'Scoped<(NoScopeRequired|OAuthForbidden)' rsky-pds/src
//! ```
//!
//! The reference implementation has a third kind that reads identically to the
//! first: an empty `authorize` body whose check actually runs in the handler,
//! distinguished only by a code comment. Here that kind is
//! `Target != ()`, so it cannot be mistaken for an opt-out and cannot be
//! declared without being checked.

use super::{
    AccessFull, AccessFullImport, AccessOutput, AccessPrivileged, AccessStandard,
    AccessStandardCheckTakedown, AccessStandardIncludeChecks, AccessStandardSignupQueued,
    AuthError, Credentials,
};
use crate::apis::ApiError;
use crate::oauth_scope::{AccountAction, RepoAction};
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome, Request};
use std::marker::PhantomData;

/// Capability token for [`AccessTier::verify`]. Its field is private to this
/// module, so no route can mint one and pull an [`AccessOutput`] out of a
/// tier directly.
pub struct Verified(());

/// A session-authentication tier: the "who is this and is the account in good
/// standing" half of a route's auth, with no scope opinion of its own.
///
/// Implemented only for the plain `Access*` guards in the parent module. Their
/// `access` field is private, so a route that names one of them as a request
/// guard authenticates and learns nothing; [`Scoped`] is the only way through.
#[rocket::async_trait]
pub trait AccessTier: Send + Sync + 'static {
    #[doc(hidden)]
    async fn verify(req: &Request<'_>, proof: Verified) -> Outcome<AccessOutput, AuthError>;
}

macro_rules! access_tier {
    ($($guard:ty),+ $(,)?) => {$(
        #[rocket::async_trait]
        impl AccessTier for $guard {
            async fn verify(
                req: &Request<'_>,
                _proof: Verified,
            ) -> Outcome<AccessOutput, AuthError> {
                match <$guard as FromRequest>::from_request(req).await {
                    Outcome::Success(guard) => Outcome::Success(guard.access),
                    Outcome::Error(error) => Outcome::Error(error),
                    Outcome::Forward(level) => Outcome::Forward(level),
                }
            }
        }
    )+};
}

access_tier!(
    AccessStandard,
    AccessStandardCheckTakedown,
    AccessStandardIncludeChecks,
    AccessStandardSignupQueued,
    AccessFull,
    AccessFullImport,
    AccessPrivileged,
);

/// What a route declares it requires of the calling session.
///
/// Every declaration states both halves explicitly, because a declaration
/// that could be half-written is a declaration that can be half-forgotten:
///
/// - [`precheck`](ScopeDecl::precheck) is what can be settled from the request
///   alone, and runs in the request guard before the handler body.
/// - [`check`](ScopeDecl::check) is what needs a value only the handler knows
///   (the collection of a record write, say), and runs when the handler asks
///   for the requester. A declaration with `Target = ()` needs nothing more
///   and writes `Ok(())`.
#[rocket::async_trait]
pub trait ScopeDecl: Send + Sync + 'static {
    /// The value this requirement is checked against. `()` when the request
    /// itself carries everything the check needs.
    type Target: Send + Sync;

    async fn precheck(req: &Request<'_>, credentials: &Option<Credentials>)
        -> Result<(), ApiError>;

    async fn check(
        credentials: &Option<Credentials>,
        target: &Self::Target,
    ) -> Result<(), ApiError>;
}

/// An authenticated session that has declared, in `D`, what it must be
/// permitted to do.
///
/// The requester's DID is reachable only from here, and only through an
/// accessor that has run `D`'s check.
///
/// A route that takes a plain access guard authenticates and learns nothing:
///
/// ```compile_fail
/// use rsky_pds::auth_verifier::AccessStandard;
///
/// fn requester(auth: AccessStandard) -> Option<String> {
///     auth.access.credentials.and_then(|c| c.did)
/// }
/// ```
///
/// A deferred declaration's handler cannot skip the value the check needs:
///
/// ```compile_fail
/// use rsky_pds::auth_verifier::scope::{RepoWrite, Scoped};
///
/// async fn requester(auth: Scoped<RepoWrite>) -> Option<String> {
///     auth.did().await.ok()
/// }
/// ```
pub struct Scoped<D: ScopeDecl, Base: AccessTier = AccessStandard> {
    access: AccessOutput,
    _decl: PhantomData<fn() -> (D, Base)>,
}

impl<D: ScopeDecl, Base: AccessTier> Clone for Scoped<D, Base> {
    fn clone(&self) -> Self {
        Self {
            access: self.access.clone(),
            _decl: PhantomData,
        }
    }
}

#[rocket::async_trait]
impl<'r, D: ScopeDecl, Base: AccessTier> FromRequest<'r> for Scoped<D, Base> {
    type Error = ApiError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match Base::verify(req, Verified(())).await {
            Outcome::Success(access) => match D::precheck(req, &access.credentials).await {
                Ok(()) => Outcome::Success(Self {
                    access,
                    _decl: PhantomData,
                }),
                Err(api_error) => {
                    req.local_cache(|| Some(api_error.clone()));
                    Outcome::Error((Status::Forbidden, api_error))
                }
            },
            Outcome::Error((status, auth_error)) => {
                let api_error = ApiError::from(&auth_error);
                req.local_cache(|| Some(api_error.clone()));
                Outcome::Error((status, api_error))
            }
            Outcome::Forward(level) => Outcome::Forward(level),
        }
    }
}

impl<D: ScopeDecl, Base: AccessTier> Scoped<D, Base> {
    /// The verified session, once `D`'s requirement has been checked against
    /// `target`.
    pub async fn credentials_for(
        &self,
        target: &D::Target,
    ) -> Result<&Option<Credentials>, ApiError> {
        D::check(&self.access.credentials, target).await?;
        Ok(&self.access.credentials)
    }

    /// The requester's DID, once `D`'s requirement has been checked against
    /// `target`.
    pub async fn did_for(&self, target: &D::Target) -> Result<String, ApiError> {
        match self
            .credentials_for(target)
            .await?
            .as_ref()
            .and_then(|c| c.did.clone())
        {
            Some(did) => Ok(did),
            None => Err(ApiError::AuthRequiredError(
                "Session carries no subject".to_string(),
            )),
        }
    }
}

/// Accessors for declarations that need nothing from the handler. The bound
/// is what keeps a deferred declaration's handler from skipping its check:
/// with `Target != ()` these methods do not exist.
impl<D: ScopeDecl<Target = ()>, Base: AccessTier> Scoped<D, Base> {
    pub async fn credentials(&self) -> Result<&Option<Credentials>, ApiError> {
        self.credentials_for(&()).await
    }

    pub async fn did(&self) -> Result<String, ApiError> {
        self.did_for(&()).await
    }

    /// The requester's DID when the route treats an unauthenticated session
    /// as anonymous rather than an error.
    pub async fn did_opt(&self) -> Result<Option<String>, ApiError> {
        Ok(self
            .credentials_for(&())
            .await?
            .as_ref()
            .and_then(|c| c.did.clone()))
    }
}

/// True for a session minted by the OAuth provider, which is exactly the set
/// of sessions that carry the proposal-0011 scope grammar. App passwords and
/// legacy `createSession` tokens carry no grants and are never OAuth.
fn is_oauth_session(credentials: &Option<Credentials>) -> bool {
    credentials
        .as_ref()
        .map(|c| c.granted_scopes.is_some())
        .unwrap_or(false)
}

// Declarations
// ------------
//
// The two opt-outs first, mirroring the reference implementation's two
// spellings of "no scope requirement here", then one declaration per
// proposal-0011 resource.

/// No OAuth scope requirement: any session this route's tier admits may call
/// it. The reference implementation spells this `authorize: () => {}`.
pub struct NoScopeRequired;

#[rocket::async_trait]
impl ScopeDecl for NoScopeRequired {
    type Target = ();

    async fn precheck(
        _req: &Request<'_>,
        _credentials: &Option<Credentials>,
    ) -> Result<(), ApiError> {
        Ok(())
    }

    async fn check(_credentials: &Option<Credentials>, _target: &()) -> Result<(), ApiError> {
        Ok(())
    }
}

/// OAuth sessions cannot reach this route at all; only app passwords and
/// legacy `createSession` tokens may call it. The reference implementation
/// spells this `authorize: () => { throw new ForbiddenError(...) }`, and uses
/// it for the account-lifecycle and credential-management surface: operations
/// that belong to the account holder acting directly, never to a client acting
/// on their behalf.
pub struct OAuthForbidden;

#[rocket::async_trait]
impl ScopeDecl for OAuthForbidden {
    type Target = ();

    async fn precheck(
        _req: &Request<'_>,
        credentials: &Option<Credentials>,
    ) -> Result<(), ApiError> {
        if is_oauth_session(credentials) {
            return Err(ApiError::InsufficientScope(
                "OAuth credentials are not supported for this endpoint".to_string(),
            ));
        }
        Ok(())
    }

    async fn check(_credentials: &Option<Credentials>, _target: &()) -> Result<(), ApiError> {
        Ok(())
    }
}

/// `blob:<mime>`, for `com.atproto.repo.uploadBlob`. The mime comes off the
/// request the same way the route's own `ContentType` guard reads it; put
/// `ContentType` ahead of this guard in a route's parameter list so a request
/// with no content type still gets that guard's rejection first.
pub struct BlobUpload;

#[rocket::async_trait]
impl ScopeDecl for BlobUpload {
    type Target = ();

    async fn precheck(
        req: &Request<'_>,
        credentials: &Option<Credentials>,
    ) -> Result<(), ApiError> {
        let mime = req
            .content_type()
            .map(ToString::to_string)
            .unwrap_or_default();
        crate::apis::assert_blob_scope(credentials, &mime)
    }

    async fn check(_credentials: &Option<Credentials>, _target: &()) -> Result<(), ApiError> {
        Ok(())
    }
}

/// `identity:handle`, for `com.atproto.identity.updateHandle`.
pub struct IdentityHandle;

#[rocket::async_trait]
impl ScopeDecl for IdentityHandle {
    type Target = ();

    async fn precheck(
        _req: &Request<'_>,
        credentials: &Option<Credentials>,
    ) -> Result<(), ApiError> {
        crate::apis::assert_identity_scope(credentials, "handle")
    }

    async fn check(_credentials: &Option<Credentials>, _target: &()) -> Result<(), ApiError> {
        Ok(())
    }
}

/// `account:email?action=manage`, for `com.atproto.server.updateEmail`.
pub struct AccountEmail;

#[rocket::async_trait]
impl ScopeDecl for AccountEmail {
    type Target = ();

    async fn precheck(
        _req: &Request<'_>,
        credentials: &Option<Credentials>,
    ) -> Result<(), ApiError> {
        crate::apis::assert_account_scope(credentials, "email", AccountAction::Manage)
    }

    async fn check(_credentials: &Option<Credentials>, _target: &()) -> Result<(), ApiError> {
        Ok(())
    }
}

/// `account:status?action=manage`, for the account activation endpoints.
pub struct AccountStatus;

#[rocket::async_trait]
impl ScopeDecl for AccountStatus {
    type Target = ();

    async fn precheck(
        _req: &Request<'_>,
        credentials: &Option<Credentials>,
    ) -> Result<(), ApiError> {
        crate::apis::assert_account_scope(credentials, "status", AccountAction::Manage)
    }

    async fn check(_credentials: &Option<Credentials>, _target: &()) -> Result<(), ApiError> {
        Ok(())
    }
}

/// `account:repo?action=manage`, for the PLC-rotation endpoints.
pub struct AccountRepo;

#[rocket::async_trait]
impl ScopeDecl for AccountRepo {
    type Target = ();

    async fn precheck(
        _req: &Request<'_>,
        credentials: &Option<Credentials>,
    ) -> Result<(), ApiError> {
        crate::apis::assert_account_scope(credentials, "repo", AccountAction::Manage)
    }

    async fn check(_credentials: &Option<Credentials>, _target: &()) -> Result<(), ApiError> {
        Ok(())
    }
}

/// One collection/action pair a handler is about to write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoTarget {
    pub collection: String,
    pub action: RepoAction,
}

impl RepoTarget {
    pub fn new(collection: impl Into<String>, action: RepoAction) -> Self {
        Self {
            collection: collection.into(),
            action,
        }
    }
}

/// `repo:<collection>`, for the record-write endpoints. Deferred, because the
/// collection is in the request body: the handler names every collection it
/// is about to touch and gets the requester back, or gets an error.
pub struct RepoWrite;

#[rocket::async_trait]
impl ScopeDecl for RepoWrite {
    type Target = Vec<RepoTarget>;

    async fn precheck(
        _req: &Request<'_>,
        _credentials: &Option<Credentials>,
    ) -> Result<(), ApiError> {
        Ok(())
    }

    async fn check(
        credentials: &Option<Credentials>,
        targets: &Vec<RepoTarget>,
    ) -> Result<(), ApiError> {
        // Declaring nothing is not a declaration; an empty list would
        // otherwise hand back the DID with no write ever checked.
        if targets.is_empty() {
            return Err(ApiError::InsufficientScope(
                "No repo write was declared for this request".to_string(),
            ));
        }
        for target in targets {
            crate::apis::assert_repo_scope(credentials, &target.collection, target.action)?;
        }
        Ok(())
    }
}

/// `rpc:<nsid>`, for the proxied-XRPC surface. The proxy target is rebuilt
/// from the request here so the check runs before the handler body, matching
/// where every other declaration runs.
pub struct RpcProxy;

#[rocket::async_trait]
impl ScopeDecl for RpcProxy {
    type Target = ();

    async fn precheck(
        req: &Request<'_>,
        credentials: &Option<Credentials>,
    ) -> Result<(), ApiError> {
        let granted_scopes = credentials.as_ref().and_then(|c| c.granted_scopes.clone());
        if granted_scopes.is_none() {
            return Ok(());
        }
        match crate::pipethrough::ProxyRequest::from_request(req).await {
            Outcome::Success(proxy_req) => {
                crate::pipethrough::assert_rpc_scope(&granted_scopes, &proxy_req).await
            }
            _ => Err(ApiError::RuntimeError),
        }
    }

    async fn check(_credentials: &Option<Credentials>, _target: &()) -> Result<(), ApiError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth_verifier::AuthScope;
    use rocket::http::ContentType;
    use rocket::local::asynchronous::Client;

    fn session(granted_scopes: Option<Vec<String>>) -> Option<Credentials> {
        Some(Credentials {
            r#type: "test".to_string(),
            did: Some("did:plc:test".to_string()),
            scope: Some(AuthScope::AppPass),
            granted_scopes,
            audience: None,
            token_id: None,
            aud: None,
            iss: None,
            is_privileged: None,
        })
    }

    fn oauth_session(granted: &[&str]) -> Option<Credentials> {
        session(Some(granted.iter().map(|s| s.to_string()).collect()))
    }

    /// App passwords and legacy `createSession` tokens carry no grants.
    fn legacy_session() -> Option<Credentials> {
        session(None)
    }

    async fn client() -> Client {
        Client::untracked(rocket::build())
            .await
            .expect("local client")
    }

    #[tokio::test]
    async fn no_scope_required_permits_every_session() {
        let client = client().await;
        let request = client.get("/");
        let req = request.inner();
        assert!(NoScopeRequired::precheck(
            req,
            &oauth_session(&["atproto", "repo:app.bsky.feed.post"])
        )
        .await
        .is_ok());
        assert!(NoScopeRequired::precheck(req, &legacy_session())
            .await
            .is_ok());
        assert!(NoScopeRequired::precheck(req, &None).await.is_ok());
    }

    #[tokio::test]
    async fn oauth_forbidden_rejects_an_oauth_session_and_permits_an_app_password() {
        let client = client().await;
        let request = client.get("/");
        let req = request.inner();
        assert!(matches!(
            OAuthForbidden::precheck(req, &oauth_session(&["atproto", "transition:generic"])).await,
            Err(ApiError::InsufficientScope(_))
        ));
        assert!(OAuthForbidden::precheck(req, &legacy_session())
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn blob_upload_reads_the_mime_off_the_request() {
        let client = client().await;
        let request = client.post("/").header(ContentType::PNG);
        assert!(BlobUpload::precheck(
            request.inner(),
            &oauth_session(&["atproto", "blob:image/*"])
        )
        .await
        .is_ok());
        let request = client.post("/").header(ContentType::Plain);
        assert!(matches!(
            BlobUpload::precheck(
                request.inner(),
                &oauth_session(&["atproto", "blob:image/*"])
            )
            .await,
            Err(ApiError::InsufficientScope(_))
        ));
    }

    #[tokio::test]
    async fn identity_and_account_declarations_narrow_their_attribute() {
        let client = client().await;
        let request = client.post("/");
        let req = request.inner();
        assert!(
            IdentityHandle::precheck(req, &oauth_session(&["atproto", "identity:handle"]))
                .await
                .is_ok()
        );
        assert!(matches!(
            IdentityHandle::precheck(req, &oauth_session(&["atproto", "identity:invalid"])).await,
            Err(ApiError::InsufficientScope(_))
        ));
        assert!(AccountEmail::precheck(
            req,
            &oauth_session(&["atproto", "account:email?action=manage"])
        )
        .await
        .is_ok());
        assert!(matches!(
            AccountEmail::precheck(req, &oauth_session(&["atproto", "account:email"])).await,
            Err(ApiError::InsufficientScope(_))
        ));
    }

    #[tokio::test]
    async fn repo_write_checks_every_declared_target() {
        let granted = oauth_session(&["atproto", "repo:app.bsky.feed.post"]);
        assert!(RepoWrite::check(
            &granted,
            &vec![RepoTarget::new("app.bsky.feed.post", RepoAction::Create)]
        )
        .await
        .is_ok());
        // One uncovered collection in a batch denies the whole batch.
        assert!(matches!(
            RepoWrite::check(
                &granted,
                &vec![
                    RepoTarget::new("app.bsky.feed.post", RepoAction::Create),
                    RepoTarget::new("app.bsky.feed.like", RepoAction::Create),
                ]
            )
            .await,
            Err(ApiError::InsufficientScope(_))
        ));
    }

    #[tokio::test]
    async fn repo_write_refuses_an_empty_declaration() {
        assert!(matches!(
            RepoWrite::check(&oauth_session(&["atproto", "repo:*"]), &vec![]).await,
            Err(ApiError::InsufficientScope(_))
        ));
    }

    #[tokio::test]
    async fn legacy_sessions_clear_every_declaration() {
        let client = client().await;
        let request = client.post("/").header(ContentType::Plain);
        let req = request.inner();
        let legacy = legacy_session();
        assert!(BlobUpload::precheck(req, &legacy).await.is_ok());
        assert!(IdentityHandle::precheck(req, &legacy).await.is_ok());
        assert!(AccountEmail::precheck(req, &legacy).await.is_ok());
        assert!(AccountStatus::precheck(req, &legacy).await.is_ok());
        assert!(AccountRepo::precheck(req, &legacy).await.is_ok());
        assert!(RepoWrite::check(
            &legacy,
            &vec![RepoTarget::new("app.bsky.feed.post", RepoAction::Create)]
        )
        .await
        .is_ok());
    }
}
