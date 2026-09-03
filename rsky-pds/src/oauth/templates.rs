use crate::oauth_scope::{parse_repo_scope, OAuthScope, RepoAction};
use crate::space_scope::{SpaceAction, SpaceScope};
use askama::Template;
use rsky_oauth::AuthorizePageData;

/// A signed-in account shown in the device account picker.
pub struct SessionOption {
    pub did: String,
    pub label: String,
}

#[derive(Template)]
#[template(path = "oauth_signin.html")]
pub struct SignInPage {
    pub client_display: String,
    pub client_id: String,
    pub request_uri: String,
    pub csrf: String,
    pub login_hint: String,
    pub error: Option<String>,
    pub signup_url: Option<String>,
    pub sessions: Vec<SessionOption>,
}

/// One granted scope, rendered on the consent screen as plain language
/// instead of the raw token a user has no way to interpret.
pub struct ScopeItem {
    /// The raw scope token, still shown (in a `<code>`) for transparency /
    /// debuggability, but never as the primary label anymore.
    pub scope: String,
    /// The plain-language headline for this grant, e.g. "Create and edit
    /// app.bsky.feed.post records".
    pub title: String,
    /// An optional second line with specifics that don't fit the headline
    /// (a collection list, a delegated audience, a permission-set caveat).
    pub detail: Option<String>,
}

#[derive(Template)]
#[template(path = "oauth_consent.html")]
pub struct ConsentPage {
    pub client_display: String,
    pub client_id: String,
    pub client_trusted: bool,
    pub request_uri: String,
    pub csrf: String,
    pub did: String,
    pub account_label: String,
    pub scopes: Vec<ScopeItem>,
}

#[derive(Template)]
#[template(path = "oauth_error.html")]
pub struct ErrorPage {
    pub message: String,
}

pub fn client_display(page: &AuthorizePageData) -> String {
    match (&page.client_name, page.client_trusted) {
        (Some(name), true) => name.clone(),
        _ => page.client_id.clone(),
    }
}

pub fn scope_items(scopes: &[String]) -> Vec<ScopeItem> {
    scopes
        .iter()
        .map(|scope| {
            let (title, detail) = describe_scope(scope);
            ScopeItem {
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
fn describe_scope(scope: &str) -> (String, Option<String>) {
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
        OAuthScope::Blob(pattern) => {
            let title = match pattern.as_str() {
                "" | "*/*" => "Upload files of any type".to_string(),
                "image/*" => "Upload images".to_string(),
                "video/*" => "Upload videos".to_string(),
                "audio/*" => "Upload audio files".to_string(),
                other => format!("Upload files matching {other}"),
            };
            (title, None)
        }
        OAuthScope::Rpc(suffix) => {
            let (nsid, aud) = match suffix.split_once('?') {
                Some((nsid, query)) => (nsid, parse_aud_param(query)),
                None => (suffix.as_str(), None),
            };
            let title = if nsid.is_empty() || nsid == "*" {
                "Call any server API on your behalf".to_string()
            } else {
                format!("Call {nsid} on your behalf")
            };
            (title, aud.map(|aud| format!("Routed through {aud}")))
        }
        // TODO(include-expansion): `include:<nsid>` names a permission set
        // published as a `com.atproto.lexicon.schema` record (see
        // `crate::permission_set`), and its real grants live in that record,
        // not in this token. Resolving it requires an async DNS + HTTP fetch
        // (`PermissionSetResolver::space_scopes`), which this synchronous,
        // per-request template render has no path to today -- the consent
        // route (`crate::oauth::routes::consent_page`) would need to expand
        // `include:` scopes into their constituent grants (mirroring
        // `permission_set::expand_includes`) *before* building `ConsentPage`,
        // so this function only ever sees the expanded, concrete scopes.
        // Until that wiring exists, show the honest limit instead of
        // pretending to know what the set contains.
        OAuthScope::Include(nsid) => (
            format!("Permissions defined by \"{nsid}\""),
            Some(
                "This app is requesting a published permission set; ask the app if you're \
                 unsure what it includes."
                    .to_string(),
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

/// `aud=...` out of a `rpc:` scope's query string, the audience the call is
/// delegated to.
fn parse_aud_param(query: &str) -> Option<String> {
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("aud="))
        .filter(|aud| !aud.is_empty())
        .map(ToString::to_string)
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

    #[test]
    fn renders_sign_in_page() {
        let page = SignInPage {
            client_display: "Example App".to_string(),
            client_id: "https://app.example.com/client".to_string(),
            request_uri: "urn:ietf:params:oauth:request_uri:req-x".to_string(),
            csrf: "csrf-token".to_string(),
            login_hint: "alice.example.com".to_string(),
            error: Some("Invalid identifier or password".to_string()),
            signup_url: Some("https://example.com/signup".to_string()),
            sessions: vec![SessionOption {
                did: "did:plc:alice".to_string(),
                label: "alice.example.com".to_string(),
            }],
        };
        let html = page.render().unwrap();
        assert!(html.contains("Example App"));
        assert!(html.contains("urn:ietf:params:oauth:request_uri:req-x"));
        assert!(html.contains("csrf-token"));
        assert!(html.contains("alice.example.com"));
        assert!(html.contains("Invalid identifier or password"));
        assert!(html.contains("https://example.com/signup"));
        assert!(html.contains("did:plc:alice"));
        assert!(html.contains("name=\"password\""));
    }

    #[test]
    fn renders_sign_in_page_without_optionals() {
        let page = SignInPage {
            client_display: "https://app.example.com/client".to_string(),
            client_id: "https://app.example.com/client".to_string(),
            request_uri: "urn:x".to_string(),
            csrf: "csrf".to_string(),
            login_hint: String::new(),
            error: None,
            signup_url: None,
            sessions: vec![],
        };
        let html = page.render().unwrap();
        assert!(html.contains("Sign in"));
        assert!(!html.contains("Create an account"));
    }

    #[test]
    fn renders_consent_page() {
        let page = ConsentPage {
            client_display: "Example App".to_string(),
            client_id: "https://app.example.com/client".to_string(),
            client_trusted: true,
            request_uri: "urn:x".to_string(),
            csrf: "csrf".to_string(),
            did: "did:plc:alice".to_string(),
            account_label: "alice.example.com".to_string(),
            scopes: scope_items(&[
                "atproto".to_string(),
                "transition:generic".to_string(),
                "transition:chat.bsky".to_string(),
                "transition:email".to_string(),
                "unknown:scope".to_string(),
            ]),
        };
        let html = page.render().unwrap();
        assert!(html.contains("Example App"));
        assert!(html.contains("alice.example.com"));
        assert!(html.contains("Confirm your identity"));
        assert!(html.contains("Access your direct messages"));
        assert!(html.contains("Additional access requested by the app"));
        assert!(html.contains("Authorize"));
        assert!(html.contains("Deny"));
    }

    /// The bug this whole module exists to fix: a modern-grammar scope like
    /// `include:app.bsky.authFull` used to render as that literal token with
    /// a generic "additional access" blurb. Every resolvable form should now
    /// carry a plain-language breakdown instead.
    #[test]
    fn renders_modern_scope_grammar_as_plain_language() {
        let page = ConsentPage {
            client_display: "Example App".to_string(),
            client_id: "https://app.example.com/client".to_string(),
            client_trusted: true,
            request_uri: "urn:x".to_string(),
            csrf: "csrf".to_string(),
            did: "did:plc:alice".to_string(),
            account_label: "alice.example.com".to_string(),
            scopes: scope_items(&[
                "repo:app.bsky.feed.post".to_string(),
                "repo:app.bsky.feed.like?action=create".to_string(),
                "blob:image/*".to_string(),
                "rpc:app.bsky.actor.getProfile?aud=did:web:api.example.com".to_string(),
                "include:app.bsky.authFull".to_string(),
                "space:app.bulleted.space?authority=*&action=read&action=create\
                 &collection=app.bulleted.note"
                    .to_string(),
            ]),
        };
        let html = page.render().unwrap();

        // repo: shows the collection and the specific actions granted, not
        // a generic "additional access" blurb.
        assert!(html.contains("Create, edit, and delete app.bsky.feed.post records"));
        assert!(html.contains("Create app.bsky.feed.like records"));

        // blob: mime pattern becomes a plain description.
        assert!(html.contains("Upload images"));

        // rpc: names the method and its delegated audience.
        assert!(html.contains("Call app.bsky.actor.getProfile on your behalf"));
        assert!(html.contains("did:web:api.example.com"));

        // include: is honest about not being expanded, rather than generic.
        assert!(html.contains("Permissions defined by &quot;app.bsky.authFull&quot;"));

        // space: breaks the grant down into who, what, and which actions.
        assert!(html.contains("Read and write in spaces shared with this app"));
        assert!(html.contains("app.bulleted.space"));
        assert!(html.contains("app.bulleted.note"));

        // The raw token is still present (in the <code> element) for anyone
        // who wants it, just no longer the *only* thing shown.
        assert!(html.contains("repo:app.bsky.feed.post"));
        assert!(html.contains("include:app.bsky.authFull"));
    }

    #[test]
    fn renders_error_page() {
        let page = ErrorPage {
            message: "this request has expired".to_string(),
        };
        let html = page.render().unwrap();
        assert!(html.contains("this request has expired"));
    }

    #[test]
    fn client_display_only_trusts_named_trusted_clients() {
        let mut data = AuthorizePageData {
            request_uri: "urn:x".to_string(),
            client_id: "https://app.example.com/client".to_string(),
            client_name: Some("Example App".to_string()),
            client_uri: None,
            logo_uri: None,
            client_trusted: false,
            scopes: vec![],
            login_hint: None,
            prompt: None,
            sessions: vec![],
        };
        assert_eq!(client_display(&data), "https://app.example.com/client");
        data.client_trusted = true;
        assert_eq!(client_display(&data), "Example App");
        data.client_name = None;
        assert_eq!(client_display(&data), "https://app.example.com/client");
    }
}
