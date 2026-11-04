//! The "Technical details" list of the consent page: one plain-language
//! line per raw scope token, whatever grammar the token uses.

use crate::oauth_scope::{
    parse_account_scope, parse_blob_scope, parse_identity_scope, parse_repo_scope, parse_rpc_scope,
    AccountAction, OAuthScope, RepoAction,
};
use crate::space_scope::{SpaceAction, SpaceScope};

/// One raw scope with its plain-language reading.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TechnicalItem {
    pub scope: String,
    pub title: String,
    pub detail: Option<String>,
}

pub fn technical_items(scopes: &[String]) -> Vec<TechnicalItem> {
    scopes
        .iter()
        .map(|scope| {
            let (title, detail) = describe_scope(scope);
            TechnicalItem {
                scope: scope.clone(),
                title,
                detail,
            }
        })
        .collect()
}

/// Turn one raw OAuth scope token into a plain-language (title, detail) pair.
///
/// Bug this replaces: the consent screen used to split the scope string on
/// whitespace and show each resulting token more or less verbatim (a bare
/// `scope_description` lookup that only recognised four literal strings and
/// otherwise fell back to "Additional access requested by the app"). A user
/// approving `include:app.bsky.authFull` never saw what that actually
/// granted. This renders every scope form this crate can currently parse
/// (`crate::oauth_scope`, `crate::space_scope`) into something a person can
/// read, instead of the wire token.
pub fn describe_scope(scope: &str) -> (String, Option<String>) {
    match OAuthScope::parse(scope) {
        OAuthScope::Atproto => (
            "Confirm your identity".to_string(),
            Some("Lets the app know who you are; grants no access on its own.".to_string()),
        ),
        OAuthScope::Transition(name) => {
            let title = match name.as_str() {
                "generic" => "Full access to your account data (except chats and email)",
                "chat.bsky" => "Access your direct messages",
                "email" => "Read your account's email address",
                _ => "Additional legacy access requested by the app",
            };
            (title.to_string(), None)
        }
        OAuthScope::Repo(suffix) => {
            let (collections, actions) = parse_repo_scope(&suffix);
            let verbs = repo_action_phrase(&actions);
            if collections.iter().any(|c| c == "*") {
                (
                    format!("{verbs} records of any type in your repository"),
                    None,
                )
            } else {
                (
                    format!("{verbs} {} records", human_list(&collections)),
                    None,
                )
            }
        }
        OAuthScope::Blob(suffix) => {
            let patterns = parse_blob_scope(&suffix);
            if patterns.is_empty() {
                // `blob:` with no pattern and no `accept=` params names no
                // mime type at all, so (unlike `repo:`'s bare form) this
                // grant permits no uploads whatsoever -- say so plainly
                // rather than implying "any type".
                (
                    "Upload files".to_string(),
                    Some(
                        "No file type was named, so this grant doesn't actually permit \
                         any uploads."
                            .to_string(),
                    ),
                )
            } else if patterns.iter().any(|p| p == "*/*") {
                ("Upload files of any type".to_string(), None)
            } else {
                let phrases: Vec<String> =
                    patterns.iter().map(|p| blob_pattern_phrase(p)).collect();
                (format!("Upload {}", human_list(&phrases)), None)
            }
        }
        OAuthScope::Rpc(suffix) => match parse_rpc_scope(&suffix) {
            Some((lxms, aud)) => {
                let title = if lxms.iter().any(|l| l == "*") {
                    "Call any server API on your behalf".to_string()
                } else {
                    format!("Call {} on your behalf", human_list(&lxms))
                };
                let detail = if aud == "*" {
                    "Routed through any service".to_string()
                } else {
                    format!("Routed through {aud}")
                };
                (title, Some(detail))
            }
            // No audience named (or the wildcard/wildcard combination the
            // parser rejects outright) -- this grant is well-formed enough
            // to recognise but confers no actual `rpc:` access
            // (`GrantedScopes::allows_rpc` never matches it), so say that
            // plainly instead of guessing at a method.
            None => (
                "Call a server API on your behalf".to_string(),
                Some(
                    "This permission doesn't name a service to route the call through, \
                     so it doesn't grant the app any access."
                        .to_string(),
                ),
            ),
        },
        OAuthScope::Identity(suffix) => match parse_identity_scope(&suffix).as_deref() {
            Some("handle") => ("Change your handle".to_string(), None),
            Some(_) => ("Change your identity attributes".to_string(), None),
            None => (
                "Change an identity attribute".to_string(),
                Some(
                    "This server could not recognise the attribute requested, so this \
                     grant doesn't actually permit any change."
                        .to_string(),
                ),
            ),
        },
        OAuthScope::Account(suffix) => match parse_account_scope(&suffix) {
            Some((attr, actions)) => {
                let manage = actions.contains(&AccountAction::Manage);
                (account_attr_phrase(&attr, manage), None)
            }
            None => (
                "Access your account settings".to_string(),
                Some(
                    "This server could not recognise this account permission, so this \
                     grant doesn't actually permit any access."
                        .to_string(),
                ),
            ),
        },
        OAuthScope::Include(nsid) => (
            format!("Permissions defined by \"{nsid}\""),
            Some(
                "A permission set published by the app; its contents are listed above.".to_string(),
            ),
        ),
        OAuthScope::Space(suffix) => describe_space_scope(&suffix),
        OAuthScope::Unknown(_) => ("Additional access requested by the app".to_string(), None),
    }
}

fn describe_space_scope(suffix: &str) -> (String, Option<String>) {
    let full = format!("{}{suffix}", crate::space_scope::SPACE_SCOPE_PREFIX);
    match SpaceScope::parse(&full) {
        Ok(parsed) => {
            let owner = match parsed.authority.as_str() {
                "self" => "your own spaces".to_string(),
                "*" => "spaces shared with this app".to_string(),
                did => format!("spaces owned by {did}"),
            };
            let verbs = space_action_phrase(parsed.actions.as_deref());
            let title = format!("{verbs} in {owner}");

            let mut detail_parts = Vec::new();
            detail_parts.push(if parsed.space_type == "*" {
                "Space type: any".to_string()
            } else {
                format!("Space type: {}", parsed.space_type)
            });
            if let Some(collections) = &parsed.collections {
                detail_parts.push(format!("Limited to: {}", collections.join(", ")));
            }
            if !parsed.manage.is_empty() {
                detail_parts.push("Can also manage the space itself".to_string());
            }
            (title, Some(detail_parts.join(" \u{b7} ")))
        }
        Err(_) => (
            "Access to a shared space".to_string(),
            Some(
                "This app is requesting a space permission this server could not fully parse."
                    .to_string(),
            ),
        ),
    }
}

/// Plain-language verb phrase for a `repo:` grant's allowed actions.
fn repo_action_phrase(actions: &[RepoAction]) -> &'static str {
    let create = actions.contains(&RepoAction::Create);
    let update = actions.contains(&RepoAction::Update);
    let delete = actions.contains(&RepoAction::Delete);
    match (create, update, delete) {
        (true, true, true) => "Create, edit, and delete",
        (true, true, false) => "Create and edit",
        (true, false, true) => "Create and delete",
        (false, true, true) => "Edit and delete",
        (true, false, false) => "Create",
        (false, true, false) => "Edit",
        (false, false, true) => "Delete",
        // `parse_repo_scope` defaults to all three when no action is named,
        // so an empty set here would mean the grammar changed underneath us;
        // fail toward showing *something* plausible rather than nothing.
        (false, false, false) => "Manage",
    }
}

/// Plain-language verb phrase for a `space:` grant's allowed actions.
fn space_action_phrase(actions: Option<&[SpaceAction]>) -> &'static str {
    let Some(actions) = actions else {
        // Omitted action list grants the full default: read, create, update,
        // delete (see `crate::space_scope`).
        return "Read and write";
    };
    let can_read = actions
        .iter()
        .any(|a| matches!(a, SpaceAction::Read | SpaceAction::ReadSelf));
    let can_write = actions.iter().any(|a| {
        matches!(
            a,
            SpaceAction::Create | SpaceAction::Update | SpaceAction::Delete
        )
    });
    match (can_read, can_write) {
        (true, true) => "Read and write",
        (true, false) => "Read",
        (false, true) => "Write",
        (false, false) => "Access",
    }
}

/// Plain-language noun phrase for one accepted `blob:` mime pattern.
fn blob_pattern_phrase(pattern: &str) -> String {
    match pattern {
        "image/*" => "images".to_string(),
        "video/*" => "videos".to_string(),
        "audio/*" => "audio files".to_string(),
        other => format!("files matching {other}"),
    }
}

/// Plain-language phrase for an `account:` grant's attribute and action.
///
/// Mirrors the surface `assert_account_scope`'s callers actually cover (see
/// `crate::apis::mod`): `email` gates reading/changing the address,
/// `status` gates activation/deactivation, and `repo` gates the
/// account-migration surface (moving the repo to another PDS) -- not
/// per-record repo writes, which are a separate `repo:` grant entirely.
fn account_attr_phrase(attr: &str, manage: bool) -> String {
    match (attr, manage) {
        ("email", true) => "Change your email address".to_string(),
        ("email", false) => "See your email address".to_string(),
        ("status", true) => "Activate or deactivate your account".to_string(),
        ("status", false) => "See your account status".to_string(),
        ("repo", true) => "Migrate or delete your account".to_string(),
        ("repo", false) => "See information about your account".to_string(),
        (other, true) => format!("Manage your account's {other}"),
        (other, false) => format!("See your account's {other}"),
    }
}

/// Join collection/scope names into a natural-language list: "a", "a and b",
/// or "a, b, and c".
fn human_list(items: &[String]) -> String {
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        2 => format!("{} and {}", items[0], items[1]),
        _ => {
            let (last, rest) = items.split_last().expect("checked len > 2 above");
            format!("{}, and {}", rest.join(", "), last)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(scopes: &[&str]) -> Vec<String> {
        technical_items(&scopes.iter().map(|s| s.to_string()).collect::<Vec<_>>())
            .into_iter()
            .map(|item| item.title)
            .collect()
    }

    fn detail(scope: &str) -> String {
        describe_scope(scope).1.unwrap_or_default()
    }

    #[test]
    fn legacy_and_unknown_scopes() {
        assert_eq!(
            titles(&[
                "atproto",
                "transition:generic",
                "transition:chat.bsky",
                "transition:email",
                "transition:other",
                "unknown:scope",
            ]),
            [
                "Confirm your identity",
                "Full access to your account data (except chats and email)",
                "Access your direct messages",
                "Read your account's email address",
                "Additional legacy access requested by the app",
                "Additional access requested by the app",
            ]
        );
        assert!(detail("atproto").contains("grants no access on its own"));
        let items = technical_items(&["atproto".to_string()]);
        assert_eq!(items[0].scope, "atproto");
    }

    #[test]
    fn modern_scope_grammar_as_plain_language() {
        assert_eq!(
            titles(&[
                "repo:app.bsky.feed.post",
                "repo:app.bsky.feed.like?action=create",
                "repo:*?action=update&action=delete",
                "repo:a.b.c?collection=d.e.f&action=create&action=delete",
                "repo:a.b.c?action=update",
                "repo:a.b.c?action=delete",
                "repo:a.b.c?action=create&action=update",
                "blob:image/*",
                "blob:?accept=image/*&accept=video/*&accept=audio/*&accept=text/plain",
                "blob:*/*",
                "blob:",
                "rpc:app.bsky.actor.getProfile?aud=did:web:api.example.com",
                "rpc:*?aud=did:web:api.example.com",
                "rpc:app.bsky.actor.getProfile?aud=*",
                "rpc:com.example.method",
                "include:app.bsky.authFull",
            ]),
            [
                "Create, edit, and delete app.bsky.feed.post records",
                "Create app.bsky.feed.like records",
                "Edit and delete records of any type in your repository",
                "Create and delete a.b.c and d.e.f records",
                "Edit a.b.c records",
                "Delete a.b.c records",
                "Create and edit a.b.c records",
                "Upload images",
                "Upload images, videos, audio files, and files matching text/plain",
                "Upload files of any type",
                "Upload files",
                "Call app.bsky.actor.getProfile on your behalf",
                "Call any server API on your behalf",
                "Call app.bsky.actor.getProfile on your behalf",
                "Call a server API on your behalf",
                "Permissions defined by \"app.bsky.authFull\"",
            ]
        );
        assert_eq!(
            detail("rpc:app.bsky.actor.getProfile?aud=did:web:api.example.com"),
            "Routed through did:web:api.example.com"
        );
        assert_eq!(
            detail("rpc:app.bsky.actor.getProfile?aud=*"),
            "Routed through any service"
        );
        assert!(detail("rpc:com.example.method").contains("doesn't grant the app any access"));
        assert!(detail("blob:").contains("doesn't actually permit any uploads"));
        assert!(detail("include:app.bsky.authFull").contains("listed above"));
        assert_eq!(repo_action_phrase(&[]), "Manage");
        assert_eq!(
            repo_action_phrase(&[RepoAction::Create, RepoAction::Delete]),
            "Create and delete"
        );
    }

    #[test]
    fn identity_and_account_scopes() {
        assert_eq!(
            titles(&[
                "identity:handle",
                "identity:*",
                "identity:bogus",
                "account:email?action=manage",
                "account:email",
                "account:status?action=manage",
                "account:status",
                "account:repo?action=manage",
                "account:repo",
                "account:bogus?action=nope",
            ]),
            [
                "Change your handle",
                "Change your identity attributes",
                "Change an identity attribute",
                "Change your email address",
                "See your email address",
                "Activate or deactivate your account",
                "See your account status",
                "Migrate or delete your account",
                "See information about your account",
                "Access your account settings",
            ]
        );
        assert!(detail("identity:bogus").contains("doesn't actually permit any change"));
        assert!(detail("account:bogus?action=nope").contains("doesn't actually permit any access"));
        assert_eq!(
            account_attr_phrase("other", true),
            "Manage your account's other"
        );
        assert_eq!(
            account_attr_phrase("other", false),
            "See your account's other"
        );
    }

    #[test]
    fn space_scopes() {
        let (title, detail) = describe_scope(
            "space:app.bulleted.space?authority=*&action=read&action=create&collection=app.bulleted.note",
        );
        assert_eq!(title, "Read and write in spaces shared with this app");
        assert_eq!(
            detail.unwrap(),
            "Space type: app.bulleted.space \u{b7} Limited to: app.bulleted.note"
        );
        let (title, detail) = describe_scope("space:*?authority=self&manage=update");
        assert_eq!(title, "Read and write in your own spaces");
        assert_eq!(
            detail.unwrap(),
            "Space type: any \u{b7} Can also manage the space itself"
        );
        let (title, _) = describe_scope("space:a.b.c?authority=did:plc:x&action=read_self");
        assert_eq!(title, "Read in spaces owned by did:plc:x");
        let (title, _) = describe_scope("space:a.b.c?authority=self&action=delete");
        assert_eq!(title, "Write in your own spaces");
        assert_eq!(space_action_phrase(Some(&[])), "Access");
        let (title, detail) = describe_scope("space:not a scope");
        assert_eq!(title, "Access to a shared space");
        assert!(detail.unwrap().contains("could not fully parse"));
    }

    #[test]
    fn human_lists() {
        assert_eq!(human_list(&[]), "");
        assert_eq!(human_list(&["a".into()]), "a");
        assert_eq!(human_list(&["a".into(), "b".into()]), "a and b");
        assert_eq!(
            human_list(&["a".into(), "b".into(), "c".into()]),
            "a, b, and c"
        );
    }
}
