//! `community.lexicon.service.describe` -- what this service is and serves.
//!
//! One unauthenticated query, no parameters. A caller learns which XRPC
//! methods this server implements without having to try each one and read the
//! failures: an unrouted method answers the same way a routed but broken one
//! does, and a caller two hops away sees neither.

use rocket::serde::json::Json;
use serde::Serialize;

/// Roles this service plays. The space methods extend what a PDS does with its
/// own repositories rather than making it a second kind of service.
const ROLES: [&str; 1] = ["pds"];

/// Every XRPC method this server routes.
///
/// Held to the mounted routes by `described_methods_match_the_mounted_routes`
/// in the integration tests: a description that overstates the surface is worse
/// than none, because a caller can handle silence.
const METHODS: [&str; 107] = [
    "app.bsky.actor.getPreferences",
    "app.bsky.actor.getProfile",
    "app.bsky.actor.getProfiles",
    "app.bsky.actor.putPreferences",
    "app.bsky.feed.getActorLikes",
    "app.bsky.feed.getAuthorFeed",
    "app.bsky.feed.getFeed",
    "app.bsky.feed.getPostThread",
    "app.bsky.feed.getTimeline",
    "app.bsky.notification.registerPush",
    "app.bsky.notification.unregisterPush",
    "com.atproto.admin.deleteAccount",
    "com.atproto.admin.disableAccountInvites",
    "com.atproto.admin.disableInviteCodes",
    "com.atproto.admin.enableAccountInvites",
    "com.atproto.admin.getAccountInfo",
    "com.atproto.admin.getAccountInfos",
    "com.atproto.admin.getInviteCodes",
    "com.atproto.admin.getSubjectStatus",
    "com.atproto.admin.sendEmail",
    "com.atproto.admin.updateAccountEmail",
    "com.atproto.admin.updateAccountHandle",
    "com.atproto.admin.updateAccountPassword",
    "com.atproto.admin.updateSubjectStatus",
    "com.atproto.identity.getRecommendedDidCredentials",
    "com.atproto.identity.refreshIdentity",
    "com.atproto.identity.requestPlcOperationSignature",
    "com.atproto.identity.resolveDid",
    "com.atproto.identity.resolveHandle",
    "com.atproto.identity.resolveIdentity",
    "com.atproto.identity.signPlcOperation",
    "com.atproto.identity.submitPlcOperation",
    "com.atproto.identity.updateHandle",
    "com.atproto.moderation.createReport",
    "com.atproto.repo.applyWrites",
    "com.atproto.repo.createRecord",
    "com.atproto.repo.deleteRecord",
    "com.atproto.repo.describeRepo",
    "com.atproto.repo.getRecord",
    "com.atproto.repo.importRepo",
    "com.atproto.repo.listMissingBlobs",
    "com.atproto.repo.listRecords",
    "com.atproto.repo.putRecord",
    "com.atproto.repo.uploadBlob",
    "com.atproto.server.activateAccount",
    "com.atproto.server.checkAccountStatus",
    "com.atproto.server.confirmEmail",
    "com.atproto.server.createAccount",
    "com.atproto.server.createAppPassword",
    "com.atproto.server.createInviteCode",
    "com.atproto.server.createInviteCodes",
    "com.atproto.server.createSession",
    "com.atproto.server.deactivateAccount",
    "com.atproto.server.deleteAccount",
    "com.atproto.server.deleteSession",
    "com.atproto.server.describeServer",
    "com.atproto.server.getAccountInviteCodes",
    "com.atproto.server.getServiceAuth",
    "com.atproto.server.getSession",
    "com.atproto.server.listAppPasswords",
    "com.atproto.server.refreshSession",
    "com.atproto.server.requestAccountDelete",
    "com.atproto.server.requestEmailConfirmation",
    "com.atproto.server.requestEmailUpdate",
    "com.atproto.server.requestPasswordReset",
    "com.atproto.server.reserveSigningKey",
    "com.atproto.server.resetPassword",
    "com.atproto.server.revokeAppPassword",
    "com.atproto.server.updateEmail",
    "com.atproto.simplespace.addMember",
    "com.atproto.simplespace.createSpace",
    "com.atproto.simplespace.deleteSpace",
    "com.atproto.simplespace.listMembers",
    "com.atproto.simplespace.removeMember",
    "com.atproto.simplespace.updateSpace",
    "com.atproto.space.applyWrites",
    "com.atproto.space.createRecord",
    "com.atproto.space.deleteRecord",
    "com.atproto.space.getBlob",
    "com.atproto.space.getDelegationToken",
    "com.atproto.space.getLatestCommit",
    "com.atproto.space.getRecord",
    "com.atproto.space.getRepo",
    "com.atproto.space.getRepoState",
    "com.atproto.space.getSpace",
    "com.atproto.space.getSpaceCredential",
    "com.atproto.space.listRecords",
    "com.atproto.space.listRepoOps",
    "com.atproto.space.listRepos",
    "com.atproto.space.listSpaces",
    "com.atproto.space.notifySpaceDeleted",
    "com.atproto.space.notifyWrite",
    "com.atproto.space.putRecord",
    "com.atproto.space.registerNotify",
    "com.atproto.sync.getBlob",
    "com.atproto.sync.getBlocks",
    "com.atproto.sync.getCheckout",
    "com.atproto.sync.getHead",
    "com.atproto.sync.getLatestCommit",
    "com.atproto.sync.getRecord",
    "com.atproto.sync.getRepo",
    "com.atproto.sync.getRepoStatus",
    "com.atproto.sync.listBlobs",
    "com.atproto.sync.listRepos",
    "com.atproto.sync.subscribeRepos",
    "com.atproto.temp.checkSignupQueue",
    "community.lexicon.service.describe",
];

/// One entry in `methods`. Every method here is standardised and resolvable by
/// NSID, so an at-uri or strongRef would only pin a schema record this server
/// does not publish.
#[derive(Debug, Serialize)]
pub struct MethodRef {
    #[serde(rename = "$type")]
    pub kind: &'static str,
    pub value: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ServiceDescription {
    pub roles: Vec<&'static str>,
    pub methods: Vec<MethodRef>,
}

/// Unauthenticated by design: a caller deciding whether it can talk to this
/// server at all has nothing to authenticate with yet.
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/community.lexicon.service.describe")]
pub async fn service_describe() -> Json<ServiceDescription> {
    Json(ServiceDescription {
        roles: ROLES.to_vec(),
        methods: METHODS
            .iter()
            .map(|nsid| MethodRef {
                kind: "community.lexicon.service.describe#nsid",
                value: nsid,
            })
            .collect(),
    })
}
