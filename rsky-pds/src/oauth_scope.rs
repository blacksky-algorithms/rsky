//! Modern atproto OAuth scope forms (permission-set proposal §Scopes).
//!
//! ```text
//! atproto | transition:<name> | repo:<nsid>[?action=...] | blob:<mime-pattern>
//!         | rpc:<nsid>[?aud=...] | include:<nsid> | space:<...>
//! ```
//!
//! This module is pure: parsing and classification only. `space:` forms are
//! delegated to [`crate::space_scope`], which owns that grammar.
//!
//! Resolution note: an `include:<nsid>` names a permission set published as a
//! `com.atproto.lexicon.schema` record, which must be fetched to learn the
//! collections and actions it confers. That resolution is not performed here,
//! so an `include:` is recognised as a well-formed permission grant but its
//! contents are not expanded. Granular enforcement therefore still happens at
//! the resource (see `crate::space_auth` for the space surface); this module
//! only answers "what kind of grant is this".

use std::fmt;

pub const SCOPE_ATPROTO: &str = "atproto";
pub const TRANSITION_PREFIX: &str = "transition:";
pub const REPO_PREFIX: &str = "repo:";
pub const BLOB_PREFIX: &str = "blob:";
pub const RPC_PREFIX: &str = "rpc:";
pub const INCLUDE_PREFIX: &str = "include:";

/// A single granted scope token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OAuthScope {
    /// The base scope every atproto session carries.
    Atproto,
    /// Legacy `transition:*` grants, which map onto the app-password model.
    Transition(String),
    /// `repo:<nsid>` — record access, optionally narrowed by `action`.
    Repo(String),
    /// `blob:<mime-pattern>` — blob upload, e.g. `image/*`.
    Blob(String),
    /// `rpc:<nsid>` — permission to call a method on another service.
    Rpc(String),
    /// `include:<nsid>` — a published permission set, resolved elsewhere.
    Include(String),
    /// `space:<...>` — permissioned-data grants; see [`crate::space_scope`].
    Space(String),
    /// Anything else. Retained rather than rejected so an unrecognised grant
    /// narrows access instead of failing the session outright.
    Unknown(String),
}

/// A repository write action named in a `repo:` scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoAction {
    Create,
    Update,
    Delete,
}

/// Parse a `repo:` scope suffix into its collections and actions, applying
/// the proposal's defaults (no collection = all, no action = all three).
///
/// `pub(crate)` rather than private: the OAuth consent screen (see
/// `crate::oauth::templates`) reuses this to describe a `repo:` grant in
/// plain language instead of duplicating the parse.
pub(crate) fn parse_repo_scope(suffix: &str) -> (Vec<String>, Vec<RepoAction>) {
    let (positional, params) = match suffix.find('?') {
        Some(pos) => (
            Some(&suffix[..pos]).filter(|p| !p.is_empty()),
            Some(&suffix[pos + 1..]),
        ),
        None => (Some(suffix).filter(|p| !p.is_empty()), None),
    };
    let mut collections: Vec<String> = Vec::new();
    if let Some(nsid) = positional {
        collections.push(nsid.to_string());
    }
    let mut actions: Vec<RepoAction> = Vec::new();
    if let Some(params) = params {
        for pair in params.split('&') {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            match key {
                "collection" if !value.is_empty() => collections.push(value.to_string()),
                "action" => match value {
                    "create" => actions.push(RepoAction::Create),
                    "update" => actions.push(RepoAction::Update),
                    "delete" => actions.push(RepoAction::Delete),
                    "*" => {
                        actions.extend([RepoAction::Create, RepoAction::Update, RepoAction::Delete])
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }
    if collections.is_empty() {
        collections.push("*".to_string());
    }
    if actions.is_empty() {
        actions.extend([RepoAction::Create, RepoAction::Update, RepoAction::Delete]);
    }
    (collections, actions)
}

impl OAuthScope {
    #[must_use]
    pub fn parse(token: &str) -> Self {
        if token == SCOPE_ATPROTO {
            return OAuthScope::Atproto;
        }
        // `repo` accepts three surface forms: bare `repo`, `repo:<nsid>`
        // shorthand, and `repo?<params>` query form.
        if token == "repo" {
            return OAuthScope::Repo(String::new());
        }
        if let Some(rest) = token.strip_prefix("repo?") {
            return OAuthScope::Repo(format!("?{rest}"));
        }
        for (prefix, ctor) in [
            (
                TRANSITION_PREFIX,
                OAuthScope::Transition as fn(String) -> OAuthScope,
            ),
            (REPO_PREFIX, OAuthScope::Repo as fn(String) -> OAuthScope),
            (BLOB_PREFIX, OAuthScope::Blob as fn(String) -> OAuthScope),
            (RPC_PREFIX, OAuthScope::Rpc as fn(String) -> OAuthScope),
            (
                INCLUDE_PREFIX,
                OAuthScope::Include as fn(String) -> OAuthScope,
            ),
        ] {
            if let Some(rest) = token.strip_prefix(prefix) {
                return ctor(rest.to_owned());
            }
        }
        if let Some(rest) = token.strip_prefix(crate::space_scope::SPACE_SCOPE_PREFIX) {
            return OAuthScope::Space(rest.to_owned());
        }
        OAuthScope::Unknown(token.to_owned())
    }

    /// Whether this token is a permission grant under the modern scope model,
    /// as opposed to the base scope or a legacy transition grant.
    #[must_use]
    pub const fn is_permission_grant(&self) -> bool {
        matches!(
            self,
            OAuthScope::Repo(_)
                | OAuthScope::Blob(_)
                | OAuthScope::Rpc(_)
                | OAuthScope::Include(_)
                | OAuthScope::Space(_)
        )
    }
}

impl fmt::Display for OAuthScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OAuthScope::Atproto => f.write_str(SCOPE_ATPROTO),
            OAuthScope::Transition(v) => write!(f, "{TRANSITION_PREFIX}{v}"),
            OAuthScope::Repo(v) => write!(f, "{REPO_PREFIX}{v}"),
            OAuthScope::Blob(v) => write!(f, "{BLOB_PREFIX}{v}"),
            OAuthScope::Rpc(v) => write!(f, "{RPC_PREFIX}{v}"),
            OAuthScope::Include(v) => write!(f, "{INCLUDE_PREFIX}{v}"),
            OAuthScope::Space(v) => {
                write!(f, "{}{v}", crate::space_scope::SPACE_SCOPE_PREFIX)
            }
            OAuthScope::Unknown(v) => f.write_str(v),
        }
    }
}

/// The granted scopes of a session, classified once.
#[derive(Debug, Clone, Default)]
pub struct GrantedScopes {
    scopes: Vec<OAuthScope>,
}

impl GrantedScopes {
    #[must_use]
    pub fn parse(granted: &[String]) -> Self {
        Self {
            scopes: granted.iter().map(|s| OAuthScope::parse(s)).collect(),
        }
    }

    #[must_use]
    pub fn has_atproto(&self) -> bool {
        self.scopes.iter().any(|s| matches!(s, OAuthScope::Atproto))
    }

    #[must_use]
    pub fn has_transition(&self, name: &str) -> bool {
        self.scopes
            .iter()
            .any(|s| matches!(s, OAuthScope::Transition(v) if v == name))
    }

    /// True when the session carries at least one modern permission grant.
    #[must_use]
    pub fn has_permission_grant(&self) -> bool {
        self.scopes.iter().any(OAuthScope::is_permission_grant)
    }

    /// Whether a granular OAuth session (one carrying `repo:`/permission
    /// grants but no `transition:generic`) governs this write. Legacy
    /// transition sessions and app passwords are `None` here and enforced by
    /// the existing scope model instead.
    #[must_use]
    pub fn is_granular_repo_session(&self) -> bool {
        !self.has_transition("generic")
            && self.scopes.iter().any(|s| matches!(s, OAuthScope::Repo(_)))
    }

    /// Whether some granted `repo:` scope permits `action` on `collection`.
    ///
    /// `repo:<nsid>` is shorthand for `repo?collection=<nsid>`; a missing
    /// collection means all collections, a missing action means all three
    /// actions, and `*` is the wildcard in either position (proposal 0016
    /// §Scopes, matching the reference `allows_repo`).
    #[must_use]
    pub fn allows_repo(&self, collection: &str, action: RepoAction) -> bool {
        self.scopes.iter().any(|s| match s {
            OAuthScope::Repo(suffix) => {
                let (collections, actions) = parse_repo_scope(suffix);
                let collection_ok = collections.iter().any(|c| c == "*" || c == collection);
                let action_ok = actions.iter().any(|a| *a == action);
                collection_ok && action_ok
            }
            _ => false,
        })
    }

    /// The `space:` grant strings, for [`crate::space_scope`] to evaluate.
    #[must_use]
    pub fn space_grants(&self) -> Vec<String> {
        self.scopes
            .iter()
            .filter_map(|s| match s {
                OAuthScope::Space(v) => {
                    Some(format!("{}{v}", crate::space_scope::SPACE_SCOPE_PREFIX))
                }
                _ => None,
            })
            .collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = &OAuthScope> {
        self.scopes.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_each_scope_form() {
        assert_eq!(OAuthScope::parse("atproto"), OAuthScope::Atproto);
        assert_eq!(
            OAuthScope::parse("transition:generic"),
            OAuthScope::Transition("generic".into())
        );
        assert_eq!(
            OAuthScope::parse("repo:app.bsky.feed.post"),
            OAuthScope::Repo("app.bsky.feed.post".into())
        );
        assert_eq!(
            OAuthScope::parse("blob:image/*"),
            OAuthScope::Blob("image/*".into())
        );
        assert_eq!(
            OAuthScope::parse("rpc:com.example.method"),
            OAuthScope::Rpc("com.example.method".into())
        );
        assert_eq!(
            OAuthScope::parse("include:app.bulleted.authFull"),
            OAuthScope::Include("app.bulleted.authFull".into())
        );
        assert_eq!(
            OAuthScope::parse("space:app.bulleted.space?action=read"),
            OAuthScope::Space("app.bulleted.space?action=read".into())
        );
        assert_eq!(
            OAuthScope::parse("something-else"),
            OAuthScope::Unknown("something-else".into())
        );
    }

    #[test]
    fn round_trips_through_display() {
        for token in [
            "atproto",
            "transition:generic",
            "repo:app.bsky.feed.post",
            "blob:image/*",
            "rpc:com.example.method",
            "include:app.bulleted.authFull",
            "space:app.bulleted.space?action=read",
            "something-else",
        ] {
            assert_eq!(OAuthScope::parse(token).to_string(), token);
        }
    }

    #[test]
    fn classifies_permission_grants() {
        assert!(!OAuthScope::parse("atproto").is_permission_grant());
        assert!(!OAuthScope::parse("transition:generic").is_permission_grant());
        assert!(OAuthScope::parse("repo:app.bsky.feed.post").is_permission_grant());
        assert!(OAuthScope::parse("blob:image/*").is_permission_grant());
        assert!(OAuthScope::parse("include:app.bulleted.authFull").is_permission_grant());
        assert!(OAuthScope::parse("space:app.bulleted.space").is_permission_grant());
        assert!(!OAuthScope::parse("nonsense").is_permission_grant());
    }

    #[test]
    fn granted_scopes_answers_session_questions() {
        // The exact base scope bulleted-app declares.
        let granted: Vec<String> = "atproto include:app.bulleted.authFull blob:image/*"
            .split_ascii_whitespace()
            .map(str::to_owned)
            .collect();
        let scopes = GrantedScopes::parse(&granted);
        assert!(scopes.has_atproto());
        assert!(!scopes.has_transition("generic"));
        assert!(scopes.has_permission_grant());
        assert!(scopes.space_grants().is_empty());
    }

    #[test]
    fn collects_space_grants_with_prefix_intact() {
        let granted: Vec<String> = vec![
            "atproto".into(),
            "space:app.bulleted.space?manage=create&action=read_self".into(),
        ];
        let scopes = GrantedScopes::parse(&granted);
        assert_eq!(
            scopes.space_grants(),
            vec!["space:app.bulleted.space?manage=create&action=read_self".to_string()]
        );
    }

    #[test]
    fn repo_scope_confines_collections_and_actions() {
        let post = "app.bsky.feed.post";
        let like = "app.bsky.feed.like";

        // A collection-limited grant permits only that collection.
        let g = GrantedScopes::parse(&["atproto".into(), format!("repo:{post}")]);
        assert!(g.is_granular_repo_session());
        assert!(g.allows_repo(post, RepoAction::Create));
        assert!(g.allows_repo(post, RepoAction::Delete));
        assert!(!g.allows_repo(like, RepoAction::Create));

        // An action-limited grant permits only that action.
        let g = GrantedScopes::parse(&["atproto".into(), format!("repo:{post}?action=create")]);
        assert!(g.allows_repo(post, RepoAction::Create));
        assert!(!g.allows_repo(post, RepoAction::Delete));

        // `repo:*` covers every collection.
        let g = GrantedScopes::parse(&["atproto".into(), "repo:*".into()]);
        assert!(g.allows_repo(like, RepoAction::Update));

        // A legacy transition session is not a granular repo session and is
        // enforced by the app-password model instead.
        let g = GrantedScopes::parse(&["atproto".into(), "transition:generic".into()]);
        assert!(!g.is_granular_repo_session());

        // Multi-valued collections in query form.
        let g = GrantedScopes::parse(&[
            "atproto".into(),
            format!("repo?collection={post}&collection={like}&action=create"),
        ]);
        assert!(g.allows_repo(post, RepoAction::Create));
        assert!(g.allows_repo(like, RepoAction::Create));
        assert!(!g.allows_repo("app.bsky.feed.repost", RepoAction::Create));
    }

    #[test]
    fn legacy_only_session_has_no_permission_grant() {
        let granted: Vec<String> = vec!["atproto".into(), "transition:generic".into()];
        let scopes = GrantedScopes::parse(&granted);
        assert!(scopes.has_atproto());
        assert!(scopes.has_transition("generic"));
        assert!(!scopes.has_permission_grant());
    }
}
