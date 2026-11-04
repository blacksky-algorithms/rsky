//! Every mutating request the pinned reference PDS accepts, with how to
//! find the account it mutates. A mutation not in this table is refused
//! for everyone, and a test fails the build when a procedure of the pinned
//! lexicon set is missing here.

use serde_json::Value;

/// How the mutation target is found in a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The `repo` field of the JSON body (a DID or a handle).
    BodyRepo,
    /// The authenticated account, from the bearer token's subject.
    AuthSubject,
    /// The `did` field of the JSON body.
    BodyDid,
    /// The `account` field of the JSON body.
    BodyAccount,
    /// The `recipientDid` field of the JSON body.
    BodyRecipientDid,
    /// The DID behind `body.token` (an email token), else `body.email`,
    /// else the bearer subject.
    TokenOrEmailOrSubject,
    /// `subject.did`, or the authority of `subject.uri`; a bare
    /// `subject.cid` is unattributable.
    SubjectStatus,
    /// The account named by `body.identifier`, or the loopback
    /// `X-Atproto-Did` header the gatekeeper sets.
    Identifier,
    /// A new account: `body.did` when the caller brings one, else none.
    NewDid,
    /// Every DID in `body.did`, a string or an array.
    BodyDidOrList,
    /// The DID behind `body.email` or `body.token`.
    EmailOrToken,
    /// No account is mutated (authorization-server traffic, invites,
    /// crawl requests).
    None,
}

/// The kinds of requests the inventory distinguishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// An XRPC procedure by NSID.
    Procedure(&'static str, Target),
    /// An OAuth UI API endpoint under `/@atproto/oauth-provider/~api`.
    Api(&'static str, Target),
}

/// The pinned inventory: every procedure of the reference lexicon set the
/// PDS implements or proxies, and every UI API endpoint.
pub const INVENTORY: &[Rule] = &[
    Rule::Procedure("com.atproto.repo.createRecord", Target::BodyRepo),
    Rule::Procedure("com.atproto.repo.putRecord", Target::BodyRepo),
    Rule::Procedure("com.atproto.repo.deleteRecord", Target::BodyRepo),
    Rule::Procedure("com.atproto.repo.applyWrites", Target::BodyRepo),
    Rule::Procedure("com.atproto.repo.uploadBlob", Target::AuthSubject),
    Rule::Procedure("com.atproto.repo.importRepo", Target::AuthSubject),
    Rule::Procedure("app.bsky.actor.putPreferences", Target::AuthSubject),
    Rule::Procedure("com.atproto.identity.updateHandle", Target::AuthSubject),
    Rule::Procedure("com.atproto.identity.refreshIdentity", Target::AuthSubject),
    Rule::Procedure(
        "com.atproto.identity.requestPlcOperationSignature",
        Target::AuthSubject,
    ),
    Rule::Procedure("com.atproto.identity.signPlcOperation", Target::AuthSubject),
    Rule::Procedure(
        "com.atproto.identity.submitPlcOperation",
        Target::AuthSubject,
    ),
    Rule::Procedure("com.atproto.server.activateAccount", Target::AuthSubject),
    Rule::Procedure("com.atproto.server.deactivateAccount", Target::AuthSubject),
    Rule::Procedure("com.atproto.server.createAppPassword", Target::AuthSubject),
    Rule::Procedure("com.atproto.server.revokeAppPassword", Target::AuthSubject),
    Rule::Procedure("com.atproto.server.confirmEmail", Target::AuthSubject),
    Rule::Procedure("com.atproto.server.requestEmailUpdate", Target::AuthSubject),
    Rule::Procedure("com.atproto.server.updateEmail", Target::AuthSubject),
    Rule::Procedure(
        "com.atproto.server.requestEmailConfirmation",
        Target::AuthSubject,
    ),
    Rule::Procedure("com.atproto.server.reserveSigningKey", Target::AuthSubject),
    Rule::Procedure("com.atproto.server.deleteAccount", Target::BodyDid),
    Rule::Procedure("com.atproto.admin.deleteAccount", Target::BodyDid),
    Rule::Procedure(
        "com.atproto.server.resetPassword",
        Target::TokenOrEmailOrSubject,
    ),
    Rule::Procedure(
        "com.atproto.server.requestPasswordReset",
        Target::TokenOrEmailOrSubject,
    ),
    Rule::Procedure(
        "com.atproto.server.requestAccountDelete",
        Target::TokenOrEmailOrSubject,
    ),
    Rule::Procedure(
        "com.atproto.admin.updateSubjectStatus",
        Target::SubjectStatus,
    ),
    Rule::Procedure("com.atproto.admin.updateAccountEmail", Target::BodyAccount),
    Rule::Procedure("com.atproto.admin.updateAccountPassword", Target::BodyDid),
    Rule::Procedure("com.atproto.admin.updateAccountHandle", Target::BodyDid),
    Rule::Procedure("com.atproto.admin.updateAccountSigningKey", Target::BodyDid),
    Rule::Procedure(
        "com.atproto.admin.enableAccountInvites",
        Target::BodyAccount,
    ),
    Rule::Procedure(
        "com.atproto.admin.disableAccountInvites",
        Target::BodyAccount,
    ),
    Rule::Procedure("com.atproto.admin.sendEmail", Target::BodyRecipientDid),
    Rule::Procedure("com.atproto.admin.disableInviteCodes", Target::None),
    Rule::Procedure("com.atproto.temp.addReservedHandle", Target::None),
    Rule::Procedure("com.atproto.temp.requestPhoneVerification", Target::None),
    Rule::Procedure(
        "com.atproto.temp.revokeAccountCredentials",
        Target::BodyAccount,
    ),
    Rule::Procedure("com.atproto.server.createSession", Target::Identifier),
    Rule::Procedure("com.atproto.server.refreshSession", Target::AuthSubject),
    Rule::Procedure("com.atproto.server.deleteSession", Target::AuthSubject),
    Rule::Procedure("com.atproto.server.createAccount", Target::NewDid),
    Rule::Procedure("com.atproto.server.createInviteCode", Target::None),
    Rule::Procedure("com.atproto.server.createInviteCodes", Target::None),
    Rule::Procedure("com.atproto.moderation.createReport", Target::AuthSubject),
    Rule::Procedure("com.atproto.sync.notifyOfUpdate", Target::None),
    Rule::Procedure("com.atproto.sync.requestCrawl", Target::None),
    // proxied to the app view or chat service; the account is the caller
    Rule::Procedure("app.bsky.ageassurance.begin", Target::AuthSubject),
    Rule::Procedure("app.bsky.bookmark.createBookmark", Target::AuthSubject),
    Rule::Procedure("app.bsky.bookmark.deleteBookmark", Target::AuthSubject),
    Rule::Procedure("app.bsky.contact.dismissMatch", Target::AuthSubject),
    Rule::Procedure("app.bsky.contact.importContacts", Target::AuthSubject),
    Rule::Procedure("app.bsky.contact.removeData", Target::AuthSubject),
    Rule::Procedure("app.bsky.contact.sendNotification", Target::AuthSubject),
    Rule::Procedure(
        "app.bsky.contact.startPhoneVerification",
        Target::AuthSubject,
    ),
    Rule::Procedure("app.bsky.contact.verifyPhone", Target::AuthSubject),
    Rule::Procedure("app.bsky.draft.createDraft", Target::AuthSubject),
    Rule::Procedure("app.bsky.draft.deleteDraft", Target::AuthSubject),
    Rule::Procedure("app.bsky.draft.updateDraft", Target::AuthSubject),
    Rule::Procedure("app.bsky.feed.sendInteractions", Target::AuthSubject),
    Rule::Procedure("app.bsky.graph.muteActor", Target::AuthSubject),
    Rule::Procedure("app.bsky.graph.muteActorList", Target::AuthSubject),
    Rule::Procedure("app.bsky.graph.muteThread", Target::AuthSubject),
    Rule::Procedure("app.bsky.graph.unmuteActor", Target::AuthSubject),
    Rule::Procedure("app.bsky.graph.unmuteActorList", Target::AuthSubject),
    Rule::Procedure("app.bsky.graph.unmuteThread", Target::AuthSubject),
    Rule::Procedure("app.bsky.unspecced.initAgeAssurance", Target::AuthSubject),
    Rule::Procedure("app.bsky.video.uploadVideo", Target::AuthSubject),
    Rule::Procedure(
        "app.bsky.notification.putActivitySubscription",
        Target::AuthSubject,
    ),
    Rule::Procedure("app.bsky.notification.putPreferences", Target::AuthSubject),
    Rule::Procedure(
        "app.bsky.notification.putPreferencesV2",
        Target::AuthSubject,
    ),
    Rule::Procedure("app.bsky.notification.registerPush", Target::AuthSubject),
    Rule::Procedure("app.bsky.notification.unregisterPush", Target::AuthSubject),
    Rule::Procedure("app.bsky.notification.updateSeen", Target::AuthSubject),
    Rule::Procedure("chat.bsky.actor.deleteAccount", Target::AuthSubject),
    Rule::Procedure("chat.bsky.convo.acceptConvo", Target::AuthSubject),
    Rule::Procedure("chat.bsky.convo.addReaction", Target::AuthSubject),
    Rule::Procedure("chat.bsky.convo.deleteMessageForSelf", Target::AuthSubject),
    Rule::Procedure("chat.bsky.convo.leaveConvo", Target::AuthSubject),
    Rule::Procedure("chat.bsky.convo.lockConvo", Target::AuthSubject),
    Rule::Procedure("chat.bsky.convo.muteConvo", Target::AuthSubject),
    Rule::Procedure("chat.bsky.convo.removeReaction", Target::AuthSubject),
    Rule::Procedure("chat.bsky.convo.sendMessage", Target::AuthSubject),
    Rule::Procedure("chat.bsky.convo.sendMessageBatch", Target::AuthSubject),
    Rule::Procedure("chat.bsky.convo.unlockConvo", Target::AuthSubject),
    Rule::Procedure("chat.bsky.convo.unmuteConvo", Target::AuthSubject),
    Rule::Procedure("chat.bsky.convo.updateAllRead", Target::AuthSubject),
    Rule::Procedure("chat.bsky.convo.updateRead", Target::AuthSubject),
    Rule::Procedure("chat.bsky.group.addMembers", Target::AuthSubject),
    Rule::Procedure("chat.bsky.group.approveJoinRequest", Target::AuthSubject),
    Rule::Procedure("chat.bsky.group.createGroup", Target::AuthSubject),
    Rule::Procedure("chat.bsky.group.createJoinLink", Target::AuthSubject),
    Rule::Procedure("chat.bsky.group.disableJoinLink", Target::AuthSubject),
    Rule::Procedure("chat.bsky.group.editGroup", Target::AuthSubject),
    Rule::Procedure("chat.bsky.group.editJoinLink", Target::AuthSubject),
    Rule::Procedure("chat.bsky.group.enableJoinLink", Target::AuthSubject),
    Rule::Procedure("chat.bsky.group.rejectJoinRequest", Target::AuthSubject),
    Rule::Procedure("chat.bsky.group.removeMembers", Target::AuthSubject),
    Rule::Procedure("chat.bsky.group.requestJoin", Target::AuthSubject),
    Rule::Procedure(
        "chat.bsky.group.updateJoinRequestsRead",
        Target::AuthSubject,
    ),
    Rule::Procedure("chat.bsky.group.withdrawJoinRequest", Target::AuthSubject),
    Rule::Procedure(
        "chat.bsky.moderation.updateActorAccess",
        Target::AuthSubject,
    ),
    Rule::Procedure("chat.bsky.notification.putPreferences", Target::AuthSubject),
    // the OAuth UI API: the managed account is `body.did`, never the session
    Rule::Api("update-handle", Target::BodyDid),
    Rule::Api("deactivate-account", Target::BodyDid),
    Rule::Api("reactivate-account", Target::BodyDid),
    Rule::Api("delete-account-request", Target::BodyDid),
    Rule::Api("delete-account-confirm", Target::BodyDid),
    Rule::Api("update-email-request", Target::BodyDid),
    Rule::Api("update-email-confirm", Target::BodyDid),
    Rule::Api("verify-email-request", Target::BodyDid),
    Rule::Api("verify-email-confirm", Target::BodyDid),
    Rule::Api("revoke-account-session", Target::BodyDid),
    Rule::Api("revoke-oauth-session", Target::BodyDid),
    Rule::Api("sign-out", Target::BodyDidOrList),
    Rule::Api("reset-password-request", Target::EmailOrToken),
    Rule::Api("reset-password-confirm", Target::EmailOrToken),
    Rule::Api("sign-in", Target::Identifier),
    Rule::Api("sign-up", Target::NewDid),
    Rule::Api("consent", Target::None),
    Rule::Api("reject", Target::None),
    Rule::Api("verify-handle-availability", Target::None),
];

/// Procedures of the pinned lexicon set the reference PDS neither
/// implements nor proxies for accounts here; they reach the moderation
/// service through the proxy and mutate nothing on this PDS.
pub const PROXIED_ELSEWHERE_PREFIXES: &[&str] = &["tools.ozone."];

pub fn procedure(nsid: &str) -> Option<Target> {
    INVENTORY.iter().find_map(|rule| match rule {
        Rule::Procedure(name, target) if *name == nsid => Some(*target),
        _ => None,
    })
}

pub fn api_endpoint(endpoint: &str) -> Option<Target> {
    INVENTORY.iter().find_map(|rule| match rule {
        Rule::Api(name, target) if *name == endpoint => Some(*target),
        _ => None,
    })
}

/// The handle or DID a field names, when it is a string.
pub fn string_field<'a>(body: &'a Value, field: &str) -> Option<&'a str> {
    body.get(field).and_then(Value::as_str)
}

/// The authority of an `at://` URI.
pub fn at_uri_authority(uri: &str) -> Option<&str> {
    let rest = uri.strip_prefix("at://")?;
    let authority = rest.split('/').next()?;
    (!authority.is_empty()).then_some(authority)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// Every procedure of the pinned lexicon set, generated from the
    /// reference tag; a new procedure upstream fails this test until it
    /// is classified.
    const PINNED_PROCEDURES: &str = include_str!("../procedures-0.5.27.txt");

    #[test]
    fn every_pinned_procedure_is_classified() {
        let classified: BTreeSet<&str> = INVENTORY
            .iter()
            .filter_map(|rule| match rule {
                Rule::Procedure(name, _) => Some(*name),
                Rule::Api(..) => None,
            })
            .collect();
        let mut missing = Vec::new();
        for nsid in PINNED_PROCEDURES.lines().filter(|line| !line.is_empty()) {
            let elsewhere = PROXIED_ELSEWHERE_PREFIXES
                .iter()
                .any(|prefix| nsid.starts_with(prefix));
            if !elsewhere && !classified.contains(nsid) {
                missing.push(nsid);
            }
        }
        assert!(missing.is_empty(), "unclassified procedures: {missing:?}");
        // and nothing classified that the pinned set does not know
        let pinned: BTreeSet<&str> = PINNED_PROCEDURES.lines().collect();
        for name in &classified {
            assert!(pinned.contains(name), "{name} is not a pinned procedure");
        }
    }

    #[test]
    fn every_ui_api_mutation_is_classified() {
        for endpoint in [
            "sign-up",
            "sign-in",
            "sign-out",
            "reset-password-request",
            "reset-password-confirm",
            "update-email-request",
            "update-email-confirm",
            "verify-email-request",
            "verify-email-confirm",
            "update-handle",
            "deactivate-account",
            "reactivate-account",
            "delete-account-request",
            "delete-account-confirm",
            "revoke-account-session",
            "revoke-oauth-session",
            "consent",
            "reject",
            "verify-handle-availability",
        ] {
            assert!(api_endpoint(endpoint).is_some(), "{endpoint}");
        }
        assert!(api_endpoint("device-sessions").is_none());
        assert_eq!(
            procedure("com.atproto.repo.createRecord"),
            Some(Target::BodyRepo)
        );
        assert!(procedure("com.example.unknown").is_none());
        assert_eq!(
            at_uri_authority("at://did:plc:a/app.bsky.feed.post/1"),
            Some("did:plc:a")
        );
        assert!(at_uri_authority("https://x").is_none());
        assert!(at_uri_authority("at:///x").is_none());
        let body = serde_json::json!({"repo": "did:plc:a", "n": 1});
        assert_eq!(string_field(&body, "repo"), Some("did:plc:a"));
        assert!(string_field(&body, "n").is_none());
    }
}
