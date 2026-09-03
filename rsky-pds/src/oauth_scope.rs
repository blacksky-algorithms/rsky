//! Modern atproto OAuth scope forms (permission-set proposal §Scopes).
//!
//! ```text
//! atproto | transition:<name> | repo:<nsid>[?action=...] | blob:<mime-pattern>
//!         | rpc:<nsid>[?aud=...] | identity:<attr> | account:<attr>[?action=...]
//!         | include:<nsid> | space:<...>
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
pub const IDENTITY_PREFIX: &str = "identity:";
pub const ACCOUNT_PREFIX: &str = "account:";
pub const INCLUDE_PREFIX: &str = "include:";

/// The `attr` values an `identity:` scope may name (proposal 0011's
/// `IdentityPermission` grammar): the account's handle, or a wildcard
/// covering every identity attribute.
pub const IDENTITY_ATTRS: [&str; 2] = ["handle", "*"];

/// The `attr` values an `account:` scope may name (proposal 0011's
/// `AccountPermission` grammar).
pub const ACCOUNT_ATTRS: [&str; 3] = ["email", "repo", "status"];

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
    /// `identity:<attr>` — permission to read or change an identity
    /// attribute (currently just the handle), or `identity:*` for all of
    /// them.
    Identity(String),
    /// `account:<attr>[?action=...]` — permission over an account-level
    /// attribute (`email`, `repo`, or `status`), narrowed to `read` or
    /// widened to `manage`.
    Account(String),
    /// `include:<nsid>` — a published permission set, resolved elsewhere.
    Include(String),
    /// `space:<...>` — permissioned-data grants; see [`crate::space_scope`].
    Space(String),
    /// Anything else: a scope string this parser does not recognise.
    ///
    /// Retained as a classified variant -- rather than making [`Self::parse`]
    /// fallible and rejecting the whole scope list -- so a token mixing
    /// recognised and unrecognised scopes still parses instead of blowing up
    /// on a form this server doesn't know about yet.
    ///
    /// It is inert everywhere a grant is evaluated: [`Self::is_permission_grant`]
    /// excludes it, and every `is_granular_*_session`/`allows_*` check on
    /// [`GrantedScopes`] matches a specific typed variant, never this one. So
    /// an unrecognised scope can never be the reason a session ends up with
    /// *more* access than its recognised scopes alone would grant -- at most
    /// it does nothing, and a session carrying only unrecognised scopes (no
    /// `transition:`, no recognised permission grant) is refused by
    /// [`crate::auth_verifier::oauth_scopes_to_auth_scope`] rather than
    /// silently treated as fully authorised.
    Unknown(String),
}

/// A repository write action named in a `repo:` scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoAction {
    Create,
    Update,
    Delete,
}

/// An action named in an `account:` scope. `Manage` implies `Read` (proposal
/// 0011's `AccountPermission.matches`: a `manage` grant also satisfies a
/// `read` check).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountAction {
    Read,
    Manage,
}

/// Parse a `repo:` scope suffix into its collections and actions, applying
/// the proposal's defaults (no collection = all, no action = all three).
fn parse_repo_scope(suffix: &str) -> (Vec<String>, Vec<RepoAction>) {
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

/// Split a scope suffix into its positional value and its query string,
/// matching the same `<positional>` / `?<params>` grammar [`parse_repo_scope`]
/// uses. Shared by the new resource kinds below; `parse_repo_scope` keeps its
/// own inline copy so its existing, reference-quality behaviour is untouched.
fn split_suffix(suffix: &str) -> (Option<&str>, Option<&str>) {
    match suffix.find('?') {
        Some(pos) => (
            Some(&suffix[..pos]).filter(|p| !p.is_empty()),
            Some(&suffix[pos + 1..]),
        ),
        None => (Some(suffix).filter(|p| !p.is_empty()), None),
    }
}

/// Parse an `identity:` scope suffix into the attribute it names.
///
/// Unlike `repo:`, an empty or unrecognised attribute is not defaulted to a
/// wildcard: proposal 0011's `IdentityPermission.attr` is required with no
/// default, so a malformed grant matches nothing rather than matching
/// everything (see the module docs on why unrecognised input must narrow,
/// never widen, access).
fn parse_identity_scope(suffix: &str) -> Option<String> {
    let (positional, params) = split_suffix(suffix);
    let attr = match (positional, params) {
        (Some(attr), None) => attr.to_string(),
        (None, Some(params)) if !params.is_empty() => {
            let mut attr = None;
            for pair in params.split('&') {
                let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                if key == "attr" && !value.is_empty() {
                    attr = Some(value.to_string());
                } else {
                    // An `identity:` grant takes no parameter but `attr`
                    // (proposal 0011 has no `action` for this resource); any
                    // other key makes the grant unrecognisable, so it must
                    // deny rather than silently ignore the stray key.
                    return None;
                }
            }
            attr?
        }
        _ => return None,
    };
    IDENTITY_ATTRS.contains(&attr.as_str()).then_some(attr)
}

/// Parse an `account:` scope suffix into the attribute and actions it names,
/// applying proposal 0011's default (`action=read` when unspecified). An
/// unrecognised attribute or action denies rather than defaulting wide, for
/// the same reason as [`parse_identity_scope`].
fn parse_account_scope(suffix: &str) -> Option<(String, Vec<AccountAction>)> {
    let (positional, params) = split_suffix(suffix);
    let mut attr: Option<String> = positional.map(str::to_string);
    let mut actions: Vec<AccountAction> = Vec::new();
    if let Some(params) = params {
        for pair in params.split('&') {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            match key {
                "attr" if attr.is_none() && !value.is_empty() => attr = Some(value.to_string()),
                "action" => match value {
                    "read" => actions.push(AccountAction::Read),
                    "manage" => actions.push(AccountAction::Manage),
                    _ => return None,
                },
                _ => return None,
            }
        }
    }
    let attr = attr?;
    if !ACCOUNT_ATTRS.contains(&attr.as_str()) {
        return None;
    }
    if actions.is_empty() {
        actions.push(AccountAction::Read);
    }
    Some((attr, actions))
}

/// Parse a `blob:` scope suffix into the mime-type patterns it accepts.
/// `blob:<pattern>` is shorthand for a single pattern; `blob?accept=...`
/// (repeated) names several. An empty suffix accepts nothing -- unlike
/// `repo:`'s bare form, there is no wildcard default here.
fn parse_blob_scope(suffix: &str) -> Vec<String> {
    let (positional, params) = split_suffix(suffix);
    let mut accepts: Vec<String> = Vec::new();
    if let Some(pattern) = positional {
        accepts.push(pattern.to_string());
    }
    if let Some(params) = params {
        for pair in params.split('&') {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            if key == "accept" && !value.is_empty() {
                accepts.push(value.to_string());
            }
        }
    }
    accepts
}

/// Whether `mime` matches an accepted pattern from a `blob:` scope, mirroring
/// [`crate::actor_store::blob::accepted_mime`]'s glob semantics (`*/*`,
/// `type/*`, or an exact match).
fn mime_matches(accepted: &[String], mime: &str) -> bool {
    accepted.iter().any(|pattern| {
        pattern == "*/*"
            || pattern == mime
            || pattern
                .strip_suffix("/*")
                .is_some_and(|base| mime.starts_with(&format!("{base}/")))
    })
}

/// Parse an `rpc:` scope suffix into the methods it names and the audience
/// it is bound to. `rpc:<nsid>` is shorthand for a single method with no
/// `aud`; the query form allows several `lxm` values plus one `aud`.
///
/// `aud` is required by proposal 0011's `RpcPermission` (no default), so a
/// grant naming no audience matches nothing. `rpc:*?aud=*` -- every method on
/// every service -- is rejected outright, matching the reference
/// implementation's constructor check.
fn parse_rpc_scope(suffix: &str) -> Option<(Vec<String>, String)> {
    let (positional, params) = split_suffix(suffix);
    let mut lxms: Vec<String> = Vec::new();
    if let Some(lxm) = positional {
        lxms.push(lxm.to_string());
    }
    let mut aud: Option<String> = None;
    if let Some(params) = params {
        for pair in params.split('&') {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            match key {
                "lxm" if !value.is_empty() => lxms.push(value.to_string()),
                "aud" if !value.is_empty() => aud = Some(value.to_string()),
                _ => {}
            }
        }
    }
    if lxms.is_empty() {
        return None;
    }
    let aud = aud?;
    if aud == "*" && lxms.iter().any(|l| l == "*") {
        return None;
    }
    Some((lxms, aud))
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
        // `identity` and `account` accept the same named-parameter query
        // form as `repo?...`, alongside their `identity:`/`account:` colon
        // shorthand handled by the prefix table below.
        if let Some(rest) = token.strip_prefix("identity?") {
            return OAuthScope::Identity(format!("?{rest}"));
        }
        if let Some(rest) = token.strip_prefix("account?") {
            return OAuthScope::Account(format!("?{rest}"));
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
                IDENTITY_PREFIX,
                OAuthScope::Identity as fn(String) -> OAuthScope,
            ),
            (
                ACCOUNT_PREFIX,
                OAuthScope::Account as fn(String) -> OAuthScope,
            ),
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
                | OAuthScope::Identity(_)
                | OAuthScope::Account(_)
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
            OAuthScope::Identity(v) => write!(f, "{IDENTITY_PREFIX}{v}"),
            OAuthScope::Account(v) => write!(f, "{ACCOUNT_PREFIX}{v}"),
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

    /// Whether a granular OAuth session governs blob uploads, mirroring
    /// [`Self::is_granular_repo_session`] for the `blob:` resource.
    #[must_use]
    pub fn is_granular_blob_session(&self) -> bool {
        !self.has_transition("generic")
            && self.scopes.iter().any(|s| matches!(s, OAuthScope::Blob(_)))
    }

    /// Whether some granted `blob:` scope accepts `mime`.
    #[must_use]
    pub fn allows_blob(&self, mime: &str) -> bool {
        self.scopes.iter().any(|s| match s {
            OAuthScope::Blob(suffix) => mime_matches(&parse_blob_scope(suffix), mime),
            _ => false,
        })
    }

    /// Whether a granular OAuth session governs calls proxied to another
    /// service, mirroring [`Self::is_granular_repo_session`] for the `rpc:`
    /// resource.
    #[must_use]
    pub fn is_granular_rpc_session(&self) -> bool {
        !self.has_transition("generic")
            && self.scopes.iter().any(|s| matches!(s, OAuthScope::Rpc(_)))
    }

    /// Whether some granted `rpc:` scope permits calling `lxm` on `aud`.
    #[must_use]
    pub fn allows_rpc(&self, lxm: &str, aud: &str) -> bool {
        self.scopes.iter().any(|s| match s {
            OAuthScope::Rpc(suffix) => match parse_rpc_scope(suffix) {
                Some((lxms, granted_aud)) => {
                    let lxm_ok = lxms.iter().any(|l| l == "*" || l == lxm);
                    let aud_ok = granted_aud == "*" || granted_aud == aud;
                    lxm_ok && aud_ok
                }
                None => false,
            },
            _ => false,
        })
    }

    /// Whether a granular OAuth session governs identity attribute changes,
    /// mirroring [`Self::is_granular_repo_session`] for the `identity:`
    /// resource.
    #[must_use]
    pub fn is_granular_identity_session(&self) -> bool {
        !self.has_transition("generic")
            && self
                .scopes
                .iter()
                .any(|s| matches!(s, OAuthScope::Identity(_)))
    }

    /// Whether some granted `identity:` scope permits acting on `attr`
    /// (`"handle"`, currently the only recognised attribute).
    #[must_use]
    pub fn allows_identity(&self, attr: &str) -> bool {
        self.scopes.iter().any(|s| match s {
            OAuthScope::Identity(suffix) => match parse_identity_scope(suffix) {
                Some(granted) => granted == "*" || granted == attr,
                None => false,
            },
            _ => false,
        })
    }

    /// Whether a granular OAuth session governs account-level mutations,
    /// mirroring [`Self::is_granular_repo_session`] for the `account:`
    /// resource.
    #[must_use]
    pub fn is_granular_account_session(&self) -> bool {
        !self.has_transition("generic")
            && self
                .scopes
                .iter()
                .any(|s| matches!(s, OAuthScope::Account(_)))
    }

    /// Whether some granted `account:` scope permits `action` on `attr`
    /// (`email`, `repo`, or `status`). A `manage` grant also satisfies a
    /// `read` check (proposal 0011 §AccountPermission).
    #[must_use]
    pub fn allows_account(&self, attr: &str, action: AccountAction) -> bool {
        self.scopes.iter().any(|s| match s {
            OAuthScope::Account(suffix) => match parse_account_scope(suffix) {
                Some((granted_attr, actions)) => {
                    granted_attr == attr
                        && (actions.contains(&AccountAction::Manage) || actions.contains(&action))
                }
                None => false,
            },
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
            OAuthScope::parse("identity:handle"),
            OAuthScope::Identity("handle".into())
        );
        assert_eq!(
            OAuthScope::parse("account:email?action=manage"),
            OAuthScope::Account("email?action=manage".into())
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
            "identity:handle",
            "identity:*",
            "account:email",
            "account:email?action=manage",
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
        assert!(OAuthScope::parse("rpc:com.example.method").is_permission_grant());
        assert!(OAuthScope::parse("identity:handle").is_permission_grant());
        assert!(OAuthScope::parse("account:email").is_permission_grant());
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

    #[test]
    fn identity_scope_parses_valid_and_rejects_invalid_forms() {
        // Positional shorthand, the primary form.
        assert_eq!(parse_identity_scope("handle"), Some("handle".to_string()));
        assert_eq!(parse_identity_scope("*"), Some("*".to_string()));
        // Named query form is equivalent.
        assert_eq!(
            parse_identity_scope("?attr=handle"),
            Some("handle".to_string())
        );
        // Unrecognised attribute, empty suffix, and a stray parameter
        // (this resource takes no `action`) all deny rather than default.
        assert_eq!(parse_identity_scope("invalid"), None);
        assert_eq!(parse_identity_scope(""), None);
        assert_eq!(parse_identity_scope("handle?action=manage"), None);
    }

    #[test]
    fn account_scope_parses_valid_and_rejects_invalid_forms() {
        assert_eq!(
            parse_account_scope("email"),
            Some(("email".to_string(), vec![AccountAction::Read]))
        );
        assert_eq!(
            parse_account_scope("email?action=manage"),
            Some(("email".to_string(), vec![AccountAction::Manage]))
        );
        assert_eq!(
            parse_account_scope("repo?action=manage"),
            Some(("repo".to_string(), vec![AccountAction::Manage]))
        );
        assert_eq!(
            parse_account_scope("?attr=status&action=manage"),
            Some(("status".to_string(), vec![AccountAction::Manage]))
        );
        // Unrecognised attribute or action, and a bare/empty suffix, deny.
        assert_eq!(parse_account_scope("invalid"), None);
        assert_eq!(parse_account_scope("email?action=invalid"), None);
        assert_eq!(parse_account_scope(""), None);
        assert_eq!(parse_account_scope("?action=manage"), None);
    }

    #[test]
    fn identity_scope_enforces_the_granted_attribute() {
        let g = GrantedScopes::parse(&["atproto".into(), "identity:handle".into()]);
        assert!(g.is_granular_identity_session());
        assert!(g.allows_identity("handle"));

        // Wildcard covers every attribute.
        let g = GrantedScopes::parse(&["atproto".into(), "identity:*".into()]);
        assert!(g.allows_identity("handle"));

        // A session without an `identity:` grant is not a granular identity
        // session (enforcement is opt-in per resource, matching the `repo:`
        // pattern).
        let g = GrantedScopes::parse(&["atproto".into(), "repo:app.bsky.feed.post".into()]);
        assert!(!g.is_granular_identity_session());

        // An invalid `identity:` grant is still classified (so it engages
        // restriction) but permits nothing.
        let g = GrantedScopes::parse(&["atproto".into(), "identity:invalid".into()]);
        assert!(g.is_granular_identity_session());
        assert!(!g.allows_identity("handle"));
    }

    #[test]
    fn account_scope_enforces_attribute_and_action() {
        let g = GrantedScopes::parse(&["atproto".into(), "account:email?action=manage".into()]);
        assert!(g.is_granular_account_session());
        assert!(g.allows_account("email", AccountAction::Manage));
        assert!(g.allows_account("email", AccountAction::Read)); // manage implies read
        assert!(!g.allows_account("status", AccountAction::Manage));

        // Default action is `read`; it does not satisfy a `manage` check.
        let g = GrantedScopes::parse(&["atproto".into(), "account:email".into()]);
        assert!(g.allows_account("email", AccountAction::Read));
        assert!(!g.allows_account("email", AccountAction::Manage));
    }

    #[test]
    fn blob_scope_enforces_the_granted_mime_pattern() {
        let g = GrantedScopes::parse(&["atproto".into(), "blob:image/*".into()]);
        assert!(g.is_granular_blob_session());
        assert!(g.allows_blob("image/png"));
        assert!(!g.allows_blob("video/mp4"));

        let g = GrantedScopes::parse(&["atproto".into(), "blob:*/*".into()]);
        assert!(g.allows_blob("video/mp4"));

        // Multiple accepted patterns via the query form.
        let g = GrantedScopes::parse(&[
            "atproto".into(),
            "blob:?accept=image/png&accept=video/mp4".into(),
        ]);
        assert!(g.allows_blob("image/png"));
        assert!(g.allows_blob("video/mp4"));
        assert!(!g.allows_blob("text/plain"));
    }

    #[test]
    fn rpc_scope_enforces_method_and_audience() {
        let g = GrantedScopes::parse(&[
            "atproto".into(),
            "rpc:com.example.method?aud=did:web:example.com".into(),
        ]);
        assert!(g.is_granular_rpc_session());
        assert!(g.allows_rpc("com.example.method", "did:web:example.com"));
        assert!(!g.allows_rpc("com.example.method", "did:web:other.com"));
        assert!(!g.allows_rpc("com.example.other", "did:web:example.com"));

        // No `aud` named: proposal 0011 requires one, so the grant matches
        // nothing rather than defaulting to "any service".
        let g = GrantedScopes::parse(&["atproto".into(), "rpc:com.example.method".into()]);
        assert!(!g.allows_rpc("com.example.method", "did:web:example.com"));

        // `rpc:*?aud=*` is forbidden outright.
        let g = GrantedScopes::parse(&["atproto".into(), "rpc:*?aud=*".into()]);
        assert!(!g.allows_rpc("com.example.method", "did:web:example.com"));
    }

    /// Regression coverage for the fail-open shape described on
    /// [`OAuthScope::Unknown`]: a scope string this parser doesn't recognise
    /// must never be the reason a session ends up with more access than its
    /// recognised scopes alone would grant.
    #[test]
    fn unrecognised_scope_never_widens_access() {
        let post = "app.bsky.feed.post";
        let with_junk = GrantedScopes::parse(&[
            "atproto".into(),
            format!("repo:{post}"),
            "totally-unrecognised-scope-string".into(),
        ]);
        let without_junk = GrantedScopes::parse(&["atproto".into(), format!("repo:{post}")]);
        // Identical restriction with or without the unrecognised scope.
        assert_eq!(
            with_junk.is_granular_repo_session(),
            without_junk.is_granular_repo_session()
        );
        assert_eq!(
            with_junk.allows_repo(post, RepoAction::Create),
            without_junk.allows_repo(post, RepoAction::Create)
        );
        assert_eq!(
            with_junk.allows_repo("app.bsky.feed.like", RepoAction::Create),
            without_junk.allows_repo("app.bsky.feed.like", RepoAction::Create)
        );
        // It doesn't unlock any *other* resource's restriction either.
        assert!(!with_junk.is_granular_blob_session());
        assert!(!with_junk.is_granular_rpc_session());
        assert!(!with_junk.is_granular_identity_session());
        assert!(!with_junk.is_granular_account_session());

        // A session carrying only unrecognised scopes (plus the mandatory
        // base scope) is not a valid modern grant and is not a granular
        // session for any resource either -- it is refused entirely by
        // `oauth_scopes_to_auth_scope` before it ever reaches these checks.
        let junk_only =
            GrantedScopes::parse(&["atproto".into(), "totally-unrecognised-scope-string".into()]);
        assert!(!junk_only.has_permission_grant());
        assert!(!junk_only.is_granular_repo_session());
        assert!(!junk_only.is_granular_blob_session());
        assert!(!junk_only.is_granular_rpc_session());
        assert!(!junk_only.is_granular_identity_session());
        assert!(!junk_only.is_granular_account_session());
    }
}
