//! The consent page's grouped description of a scope list, following the
//! reference authorization UI: one card per kind of access (email,
//! identity, account, the deployment's app, chat, each included permission
//! set, then the fine-grained repository and RPC tables) so the user reads
//! what an app can do rather than the wire tokens.

use crate::oauth_scope::{
    parse_account_scope, parse_blob_scope, parse_identity_scope, parse_repo_scope, parse_rpc_scope,
    AccountAction, OAuthScope, RepoAction,
};
use crate::space_scope::SpaceScope;
use std::collections::BTreeMap;

/// A resolved `include:` set as the page needs it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IncludeSetView {
    pub title: Option<String>,
    pub detail: Option<String>,
    pub scopes: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoRow {
    pub collection: String,
    pub create: bool,
    pub update: bool,
    pub delete: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoTable {
    pub rows: Vec<RepoRow>,
    pub blobs: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RpcRow {
    /// The audience as the page shows it
    pub aud_label: String,
    /// The raw audience, for the tooltip
    pub aud: String,
    /// `true` when the label is prose ("Any service"), false for an identifier
    pub aud_is_prose: bool,
    pub lxms: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RpcTable {
    pub rows: Vec<RpcRow>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PermissionGroup {
    /// The name of an inline icon partial
    pub icon: &'static str,
    pub title: String,
    /// May carry `<b>` emphasis; rendered unescaped
    pub description: String,
    pub intro: Option<String>,
    pub repo_table: Option<RepoTable>,
    pub rpc_table: Option<RpcTable>,
    /// The email card offers a checkbox to decline that grant
    pub email_checkbox: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Grouping {
    pub groups: Vec<PermissionGroup>,
    pub identity_warning: bool,
    /// `account:email` is present and can be declined inline
    pub can_drop_email: bool,
    pub only_atproto: bool,
}

struct RepoGrant {
    collections: Vec<String>,
    actions: Vec<RepoAction>,
}

struct RpcGrant {
    lxms: Vec<String>,
    aud: String,
}

/// The parsed view of a scope list the grouping rules consult.
#[derive(Default)]
struct Permissions {
    generic: bool,
    chat_transition: bool,
    email_transition: bool,
    repo: Vec<RepoGrant>,
    rpc: Vec<RpcGrant>,
    blob: bool,
    account: Vec<(String, Vec<AccountAction>)>,
    identity: Vec<String>,
    space: Vec<String>,
    unknown: Vec<String>,
    only_atproto: bool,
}

impl Permissions {
    fn from_scopes(scopes: &[String]) -> Self {
        let mut p = Permissions::default();
        let mut saw_other = false;
        for scope in scopes {
            match OAuthScope::parse(scope) {
                OAuthScope::Atproto => {}
                OAuthScope::Transition(name) => {
                    saw_other = true;
                    match name.as_str() {
                        "generic" => p.generic = true,
                        "chat.bsky" => p.chat_transition = true,
                        "email" => p.email_transition = true,
                        _ => {}
                    }
                }
                OAuthScope::Repo(suffix) => {
                    saw_other = true;
                    let (collections, actions) = parse_repo_scope(&suffix);
                    p.repo.push(RepoGrant {
                        collections,
                        actions,
                    });
                }
                OAuthScope::Blob(suffix) => {
                    saw_other = true;
                    if !parse_blob_scope(&suffix).is_empty() {
                        p.blob = true;
                    }
                }
                OAuthScope::Rpc(suffix) => {
                    saw_other = true;
                    if let Some((lxms, aud)) = parse_rpc_scope(&suffix) {
                        p.rpc.push(RpcGrant { lxms, aud });
                    }
                }
                OAuthScope::Identity(suffix) => {
                    saw_other = true;
                    if let Some(attr) = parse_identity_scope(&suffix) {
                        p.identity.push(attr);
                    }
                }
                OAuthScope::Account(suffix) => {
                    saw_other = true;
                    if let Some(grant) = parse_account_scope(&suffix) {
                        p.account.push(grant);
                    }
                }
                OAuthScope::Include(_) => saw_other = true,
                OAuthScope::Space(suffix) => {
                    saw_other = true;
                    p.space.push(suffix);
                }
                OAuthScope::Unknown(raw) => {
                    saw_other = true;
                    p.unknown.push(raw);
                }
            }
        }
        p.only_atproto = !saw_other;
        p
    }

    fn allows_account(&self, attr: &str, manage: bool) -> bool {
        self.account.iter().any(|(a, actions)| {
            a == attr
                && (actions.contains(&AccountAction::Manage)
                    || (!manage && actions.contains(&AccountAction::Read)))
        })
    }

    fn allows_identity(&self, attr: &str) -> bool {
        self.identity.iter().any(|a| a == "*" || a == attr)
    }

    fn allows_repo(&self, collection: &str, action: RepoAction) -> bool {
        self.repo.iter().any(|g| {
            g.collections.iter().any(|c| c == "*" || c == collection) && g.actions.contains(&action)
        })
    }

    fn broad_repo(&self) -> bool {
        self.generic
            || self.allows_repo("*", RepoAction::Create)
            || self.allows_repo("*", RepoAction::Update)
            || self.allows_repo("*", RepoAction::Delete)
    }

    fn has_rpc(&self) -> bool {
        !self.rpc.is_empty()
    }

    fn has_repo(&self) -> bool {
        !self.repo.is_empty()
    }

    /// Every repo/rpc grant targets the social app's own lexicons, so the app
    /// card covers them and the generic tables would repeat it.
    fn only_app_specific(&self) -> bool {
        if self.allows_account("repo", true) {
            return false;
        }
        let mut found = false;
        for grant in &self.rpc {
            found = true;
            if is_official_appview(&grant.aud) {
                continue;
            }
            if grant.lxms.iter().all(|l| is_app_specific_nsid(l)) {
                continue;
            }
            return false;
        }
        for grant in &self.repo {
            found = true;
            if grant.collections.iter().all(|c| is_app_specific_nsid(c)) {
                continue;
            }
            return false;
        }
        found
    }

    fn enables_app_repo(&self) -> bool {
        self.generic
            || self.repo.iter().any(|g| {
                g.collections
                    .iter()
                    .any(|c| c == "*" || is_app_specific_nsid(c))
            })
    }

    fn enables_private_app_methods(&self) -> bool {
        self.generic
            || self.rpc.iter().any(|g| {
                g.lxms
                    .iter()
                    .any(|l| l == "*" || PRIVATE_APP_METHODS.contains(&l.as_str()))
            })
    }

    fn enables_chat(&self) -> bool {
        self.chat_transition
            || self.rpc.iter().any(|g| {
                !is_official_appview(&g.aud)
                    && g.lxms
                        .iter()
                        .any(|l| l == "*" || l.starts_with("chat.bsky."))
            })
    }

    fn repo_table(&self) -> Option<RepoTable> {
        let mut rows: BTreeMap<String, RepoRow> = BTreeMap::new();
        for grant in &self.repo {
            for collection in &grant.collections {
                let row = rows.entry(collection.clone()).or_insert_with(|| RepoRow {
                    collection: collection.clone(),
                    ..RepoRow::default()
                });
                for action in &grant.actions {
                    match action {
                        RepoAction::Create => row.create = true,
                        RepoAction::Update => row.update = true,
                        RepoAction::Delete => row.delete = true,
                    }
                }
            }
        }
        if rows.is_empty() {
            return None;
        }
        let star = rows.get("*").cloned();
        let rows = rows
            .into_values()
            .map(|mut row| {
                if let Some(star) = &star {
                    row.create |= star.create;
                    row.update |= star.update;
                    row.delete |= star.delete;
                }
                row
            })
            .collect();
        Some(RepoTable {
            rows,
            blobs: self.blob || self.generic,
        })
    }

    fn rpc_table(&self) -> Option<RpcTable> {
        let mut by_aud: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for grant in &self.rpc {
            let entry = by_aud.entry(grant.aud.clone()).or_default();
            for lxm in &grant.lxms {
                if !entry.contains(lxm) {
                    entry.push(lxm.clone());
                }
            }
        }
        if by_aud.is_empty() {
            return None;
        }
        let rows = by_aud
            .into_iter()
            .map(|(aud, mut lxms)| {
                if lxms.iter().any(|l| l == "*") {
                    lxms = vec!["*".to_string()];
                } else {
                    lxms.sort();
                }
                let (aud_label, aud_is_prose) = aud_label(&aud);
                RpcRow {
                    aud_label,
                    aud,
                    aud_is_prose,
                    lxms,
                }
            })
            .collect();
        Some(RpcTable { rows })
    }
}

const PRIVATE_APP_METHODS: &[&str] = &[
    "app.bsky.actor.getPreferences",
    "app.bsky.graph.block",
    "app.bsky.graph.muteActor",
    "app.bsky.graph.muteActorList",
    "app.bsky.graph.muteThread",
    "app.bsky.graph.unmuteActor",
    "app.bsky.graph.unmuteActorList",
    "app.bsky.graph.unmuteThread",
    "app.bsky.graph.getMutes",
];

fn is_official_appview(aud: &str) -> bool {
    aud == "did:web:bsky.app#bsky_appview"
}

fn is_app_specific_nsid(nsid: &str) -> bool {
    nsid != "*"
        && (nsid == "com.atproto.moderation.createReport"
            || nsid.starts_with("app.bsky.")
            || nsid.starts_with("chat.bsky."))
}

/// How an RPC audience reads on the page: prose for wildcards and
/// `did:web` services, the identifier otherwise.
pub fn aud_label(aud: &str) -> (String, bool) {
    if aud == "*" {
        return ("Any service".to_string(), true);
    }
    if let Some(rest) = aud.strip_prefix("did:web:") {
        if let Some((host, _)) = rest.split_once('#') {
            return (
                format!("A service controlled by <b>{}</b>", escape(host)),
                true,
            );
        }
    }
    (escape(aud), false)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const INTRO_ON_BEHALF: &str =
    "The application requests the permissions necessary to perform the following actions on your behalf:";
const INTRO_REPO: &str = "Your repository contains all the data publicly available on the AT Protocol network, such as posts, likes, and follows. It also contains data created through other apps you've signed into using this account. The application requests the permissions necessary to publish the following changes to your repository:";
const INTRO_RPC: &str = "The AT Protocol network uses an authentication mechanism that allows to uniquely identify users when communicating with external services. This is typically used to retrieve or update data linked to your account, such as chat messages, feeds or moderation content. The application requests the permissions necessary to perform the following authenticated actions on your behalf:";

/// Build the grouped view. `include_sets` maps each `include:<nsid>` token in
/// `scopes` to its resolved set; a token missing from the map is shown by its
/// NSID (the caller has already decided resolution failures abort the flow).
/// `first_party` suppresses the identity-takeover warning, as the reference
/// does for clients that are both trusted and the deployment's own.
pub fn permission_groups(
    scopes: &[String],
    include_sets: &BTreeMap<String, IncludeSetView>,
    first_party: bool,
    app_name: Option<&str>,
) -> Grouping {
    let p = Permissions::from_scopes(scopes);
    let mut grouping = Grouping {
        only_atproto: p.only_atproto,
        ..Grouping::default()
    };
    if p.only_atproto {
        return grouping;
    }
    let app_name = app_name.unwrap_or("Social app");
    let any_transition = p.generic || p.chat_transition || p.email_transition;

    // Email
    let email_manage = p.allows_account("email", true) || p.email_transition;
    let email_read = p.allows_account("email", false) || p.email_transition;
    if email_manage || email_read {
        grouping.can_drop_email = !any_transition && !p.email_transition;
        grouping.groups.push(PermissionGroup {
            icon: "mail",
            title: "Email".into(),
            description: if email_manage {
                "Read and update your account's email address".into()
            } else {
                "Read your account's email address".into()
            },
            email_checkbox: grouping.can_drop_email,
            ..PermissionGroup::default()
        });
    }

    // Identity
    if p.allows_identity("*") {
        grouping.identity_warning = !first_party;
        grouping.groups.push(PermissionGroup {
            icon: "id-card",
            title: "Identity".into(),
            description: "Manage your <b>full identity</b> including your <b>@handle</b>".into(),
            ..PermissionGroup::default()
        });
    } else if p.allows_identity("handle") {
        grouping.groups.push(PermissionGroup {
            icon: "id-card",
            title: "Identity".into(),
            description: "Change your <b>@handle</b>".into(),
            ..PermissionGroup::default()
        });
    }

    // Account status
    if p.allows_account("status", true) {
        grouping.groups.push(PermissionGroup {
            icon: "user",
            title: "Account".into(),
            description: "Temporarily activate or deactivate your account".into(),
            ..PermissionGroup::default()
        });
    }

    // The deployment's app
    let app_repo = p.enables_app_repo();
    let app_rpc = p.enables_private_app_methods();
    let broad = p.broad_repo();
    let only_app_specific = p.only_app_specific();
    if app_repo || app_rpc {
        let description = match (broad, app_rpc, app_repo) {
            (true, true, _) => "Manage your profile, posts, likes and follows as well as read your private preferences".to_string(),
            (true, false, _) => "Manage your profile, posts, likes and follows".to_string(),
            (false, true, true) => format!("Access specific parts of your {} account and read your private preferences", escape(app_name)),
            (false, false, true) => format!("Access specific parts of your {} account", escape(app_name)),
            (false, _, false) => "Read your private preferences".to_string(),
        };
        let show_details = !broad && only_app_specific;
        grouping.groups.push(PermissionGroup {
            icon: "layout-grid",
            title: app_name.to_string(),
            description,
            intro: show_details.then(|| {
                format!(
                    "The application requests the permissions necessary to perform the following actions on your {} account:",
                    escape(app_name)
                )
            }),
            rpc_table: if show_details { p.rpc_table() } else { None },
            repo_table: if show_details { p.repo_table() } else { None },
            ..PermissionGroup::default()
        });
    }

    // Chat
    if p.enables_chat() {
        grouping.groups.push(PermissionGroup {
            icon: "message-circle-more",
            title: "Chat".into(),
            description: "Read and send messages".into(),
            ..PermissionGroup::default()
        });
    }

    // Included permission sets
    for scope in scopes {
        if let OAuthScope::Include(nsid) = OAuthScope::parse(scope) {
            let set = include_sets.get(scope).or_else(|| include_sets.get(&nsid));
            let icon = if nsid.starts_with("app.bsky.") {
                "layout-grid"
            } else if nsid.starts_with("chat.bsky.") {
                "message-circle-more"
            } else if nsid.starts_with("com.atproto.moderation.") {
                "hand"
            } else {
                "atom"
            };
            let (title, description) = match set.and_then(|s| s.title.clone()) {
                Some(title) => (
                    escape(&title),
                    set.and_then(|s| s.detail.clone())
                        .map(|d| escape(&d))
                        .unwrap_or_else(|| escape(&nsid)),
                ),
                None => (
                    escape(&nsid),
                    set.and_then(|s| s.detail.clone())
                        .map(|d| escape(&d))
                        .unwrap_or_default(),
                ),
            };
            let inner = set
                .map(|s| Permissions::from_scopes(&s.scopes))
                .unwrap_or_default();
            grouping.groups.push(PermissionGroup {
                icon,
                title,
                description,
                intro: Some(INTRO_ON_BEHALF.to_string()),
                rpc_table: inner.rpc_table(),
                repo_table: inner.repo_table(),
                ..PermissionGroup::default()
            });
        }
    }

    // Fine-grained repository and RPC access
    if !only_app_specific {
        if p.generic
            || p.allows_account("repo", true)
            || (p.allows_repo("*", RepoAction::Create)
                && p.allows_repo("*", RepoAction::Update)
                && p.allows_repo("*", RepoAction::Delete))
        {
            grouping.groups.push(PermissionGroup {
                icon: "book-open",
                title: "Repository".into(),
                description: "Create, update, and delete any public data linked to your account"
                    .into(),
                ..PermissionGroup::default()
            });
        } else if p.has_repo() {
            grouping.groups.push(PermissionGroup {
                icon: "book-open",
                title: "Repository".into(),
                description: "Publish changes".into(),
                intro: Some(INTRO_REPO.to_string()),
                repo_table: p.repo_table(),
                ..PermissionGroup::default()
            });
        }
        if p.generic {
            grouping.groups.push(PermissionGroup {
                icon: "badge-check",
                title: "Authenticate".into(),
                description: "Perform authenticated actions towards <b>any service</b> on your behalf".into(),
                ..PermissionGroup::default()
            });
        } else if p.has_rpc() {
            grouping.groups.push(PermissionGroup {
                icon: "badge-check",
                title: "Authenticate".into(),
                description: "Perform actions on your behalf".into(),
                intro: Some(INTRO_RPC.to_string()),
                rpc_table: p.rpc_table(),
                ..PermissionGroup::default()
            });
        }
    }

    // rsky's spaces
    for suffix in &p.space {
        let full = format!("{}{suffix}", crate::space_scope::SPACE_SCOPE_PREFIX);
        let description = match SpaceScope::parse(&full) {
            Ok(parsed) => {
                let owner = match parsed.authority.as_str() {
                    "self" => "your own spaces".to_string(),
                    "*" => "spaces shared with this app".to_string(),
                    did => format!("spaces owned by <b>{}</b>", escape(did)),
                };
                let kind = if parsed.space_type == "*" {
                    "of any type".to_string()
                } else {
                    format!("of type <b>{}</b>", escape(&parsed.space_type))
                };
                format!("Read and write in {owner} {kind}")
            }
            Err(_) => "Access to a shared space this server could not fully parse".to_string(),
        };
        grouping.groups.push(PermissionGroup {
            icon: "atom",
            title: "Spaces".into(),
            description,
            ..PermissionGroup::default()
        });
    }

    grouping
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_transition_scope_is_not_a_group_of_its_own() {
        let g = permission_groups(
            &["atproto".to_string(), "transition:other".to_string()],
            &BTreeMap::new(),
            false,
            None,
        );
        assert!(g.groups.is_empty());
        assert!(!g.only_atproto);
    }

    fn scopes(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn group(list: &[&str]) -> Grouping {
        permission_groups(&scopes(list), &BTreeMap::new(), false, Some("Blacksky"))
    }

    fn titles(g: &Grouping) -> Vec<String> {
        g.groups.iter().map(|x| x.title.clone()).collect()
    }

    #[test]
    fn atproto_alone_shows_nothing() {
        let g = group(&["atproto"]);
        assert!(g.only_atproto);
        assert!(g.groups.is_empty());
        assert!(!group(&["atproto", "transition:generic"]).only_atproto);
    }

    #[test]
    fn generic_transition_is_the_broad_grant() {
        let g = group(&["atproto", "transition:generic"]);
        assert_eq!(titles(&g), vec!["Blacksky", "Repository", "Authenticate"]);
        assert_eq!(
            g.groups[0].description,
            "Manage your profile, posts, likes and follows as well as read your private preferences"
        );
        assert!(g.groups[0].intro.is_none());
        assert!(g.groups[2].description.contains("<b>any service</b>"));
        assert!(!g.can_drop_email);
    }

    #[test]
    fn email_identity_account_and_chat_cards() {
        let g = group(&[
            "atproto",
            "account:email?action=manage",
            "identity:*",
            "account:status?action=manage",
            "transition:chat.bsky",
        ]);
        assert_eq!(titles(&g), vec!["Email", "Identity", "Account", "Chat"]);
        assert_eq!(
            g.groups[0].description,
            "Read and update your account's email address"
        );
        // a transition scope is present, so the email grant cannot be declined
        assert!(!g.can_drop_email);
        assert!(!g.groups[0].email_checkbox);
        assert!(g.identity_warning);
        assert!(g.groups[1].description.contains("full identity"));

        let g = group(&["atproto", "account:email", "identity:handle"]);
        assert_eq!(g.groups[0].description, "Read your account's email address");
        assert!(g.can_drop_email);
        assert!(g.groups[0].email_checkbox);
        assert!(!g.identity_warning);
        assert_eq!(g.groups[1].description, "Change your <b>@handle</b>");

        let trusted_first_party = permission_groups(
            &scopes(&["atproto", "identity:*"]),
            &BTreeMap::new(),
            true,
            None,
        );
        assert!(!trusted_first_party.identity_warning);
        assert_eq!(
            group(&["atproto", "transition:email"]).groups[0].description,
            "Read and update your account's email address"
        );
    }

    #[test]
    fn app_specific_repo_grants_stay_inside_the_app_card() {
        let g = group(&[
            "atproto",
            "repo:app.bsky.feed.post?action=create&action=delete",
            "repo:app.bsky.feed.like",
            "blob:image/*",
            "rpc:app.bsky.actor.getPreferences?aud=did:web:bsky.app#bsky_appview",
        ]);
        assert_eq!(titles(&g), vec!["Blacksky"]);
        let app = &g.groups[0];
        assert_eq!(
            app.description,
            "Access specific parts of your Blacksky account and read your private preferences"
        );
        assert_eq!(
            app.intro.as_deref(),
            Some("The application requests the permissions necessary to perform the following actions on your Blacksky account:")
        );
        let repo = app.repo_table.as_ref().unwrap();
        assert_eq!(repo.rows.len(), 2);
        assert_eq!(repo.rows[0].collection, "app.bsky.feed.like");
        assert!(repo.rows[0].create && repo.rows[0].update && repo.rows[0].delete);
        assert_eq!(repo.rows[1].collection, "app.bsky.feed.post");
        assert!(repo.rows[1].create && !repo.rows[1].update && repo.rows[1].delete);
        assert!(repo.blobs);
        let rpc = app.rpc_table.as_ref().unwrap();
        assert_eq!(rpc.rows[0].aud, "did:web:bsky.app#bsky_appview");
        assert_eq!(
            rpc.rows[0].aud_label,
            "A service controlled by <b>bsky.app</b>"
        );
        assert!(rpc.rows[0].aud_is_prose);
        assert_eq!(rpc.rows[0].lxms, vec!["app.bsky.actor.getPreferences"]);
    }

    #[test]
    fn generic_collections_get_the_fine_grained_cards() {
        let g = group(&[
            "atproto",
            "repo:com.example.thing?action=create",
            "rpc:com.example.method?aud=*",
            "rpc:com.example.other?aud=did:plc:abc",
        ]);
        assert_eq!(titles(&g), vec!["Repository", "Authenticate"]);
        assert_eq!(g.groups[0].description, "Publish changes");
        assert!(g.groups[0]
            .intro
            .as_deref()
            .unwrap()
            .starts_with("Your repository contains"));
        let rpc = g.groups[1].rpc_table.as_ref().unwrap();
        assert_eq!(rpc.rows.len(), 2);
        assert_eq!(rpc.rows[0].aud_label, "Any service");
        assert_eq!(rpc.rows[1].aud_label, "did:plc:abc");
        assert!(!rpc.rows[1].aud_is_prose);

        let g = group(&["atproto", "repo:*"]);
        assert_eq!(titles(&g), vec!["Blacksky", "Repository"]);
        assert_eq!(
            g.groups[0].description,
            "Manage your profile, posts, likes and follows"
        );
        assert_eq!(
            g.groups[1].description,
            "Create, update, and delete any public data linked to your account"
        );

        let g = group(&[
            "atproto",
            "account:repo?action=manage",
            "rpc:*?aud=did:web:x.test#svc",
        ]);
        assert_eq!(
            titles(&g),
            vec!["Blacksky", "Chat", "Repository", "Authenticate"]
        );
        assert_eq!(
            g.groups[3].rpc_table.as_ref().unwrap().rows[0].lxms,
            vec!["*"]
        );
    }

    #[test]
    fn chat_from_rpc_and_a_star_row_merges_into_every_collection() {
        let g = group(&[
            "atproto",
            "rpc:chat.bsky.convo.getMessages?aud=did:web:api.bsky.chat#bsky_chat",
        ]);
        assert_eq!(titles(&g), vec!["Chat"]);

        let g = group(&[
            "atproto",
            "repo:*?action=update",
            "repo:com.example.a?action=create",
        ]);
        let repo = g.groups.iter().find(|x| x.title == "Repository").unwrap();
        let table = repo.repo_table.as_ref().unwrap();
        let a = table
            .rows
            .iter()
            .find(|r| r.collection == "com.example.a")
            .unwrap();
        assert!(a.create && a.update && !a.delete);
        assert!(table.rows.iter().any(|r| r.collection == "*"));
        assert!(!table.blobs);
    }

    #[test]
    fn included_sets_render_from_their_definition() {
        let mut sets = BTreeMap::new();
        sets.insert(
            "include:app.bulleted.spaceAccess".to_string(),
            IncludeSetView {
                title: Some("Bulleted <spaces>".into()),
                detail: Some("Read the outlines.".into()),
                scopes: scopes(&["repo:app.bulleted.note?action=create"]),
            },
        );
        let g = permission_groups(
            &scopes(&[
                "atproto",
                "include:app.bulleted.spaceAccess",
                "include:chat.bsky.auth",
                "include:com.atproto.moderation.tools",
            ]),
            &sets,
            false,
            None,
        );
        assert_eq!(
            titles(&g),
            vec![
                "Bulleted &lt;spaces&gt;",
                "chat.bsky.auth",
                "com.atproto.moderation.tools"
            ]
        );
        assert_eq!(g.groups[0].icon, "atom");
        assert_eq!(g.groups[0].description, "Read the outlines.");
        assert_eq!(g.groups[0].intro.as_deref(), Some(INTRO_ON_BEHALF));
        assert_eq!(
            g.groups[0].repo_table.as_ref().unwrap().rows[0].collection,
            "app.bulleted.note"
        );
        assert_eq!(g.groups[1].icon, "message-circle-more");
        assert_eq!(g.groups[1].description, "");
        assert_eq!(g.groups[2].icon, "hand");
        let by_nsid: BTreeMap<String, IncludeSetView> = [(
            "app.bsky.authFull".to_string(),
            IncludeSetView {
                title: Some("Full".into()),
                detail: None,
                scopes: vec![],
            },
        )]
        .into();
        let g = permission_groups(
            &scopes(&["atproto", "include:app.bsky.authFull"]),
            &by_nsid,
            false,
            None,
        );
        assert_eq!(g.groups[0].icon, "layout-grid");
        assert_eq!(g.groups[0].title, "Full");
        assert_eq!(g.groups[0].description, "app.bsky.authFull");
    }

    #[test]
    fn spaces_and_unknown_scopes() {
        let g = group(&[
            "atproto",
            "space:app.bulleted.space?authority=*",
            "space:not a scope",
            "something:weird",
        ]);
        assert_eq!(titles(&g), vec!["Spaces", "Spaces"]);
        assert!(g.groups[0]
            .description
            .contains("spaces shared with this app"));
        assert!(g.groups[0]
            .description
            .contains("<b>app.bulleted.space</b>"));
        assert!(g.groups[1].description.contains("could not fully parse"));
        let g = group(&["atproto", "space:*?authority=did:plc:owner"]);
        assert!(g.groups[0]
            .description
            .contains("spaces owned by <b>did:plc:owner</b>"));
        assert!(g.groups[0].description.contains("of any type"));
        let g = group(&["atproto", "space:app.x.space?authority=self"]);
        assert!(g.groups[0].description.contains("your own spaces"));
    }

    #[test]
    fn aud_labels_and_app_name_fallback() {
        assert_eq!(aud_label("*"), ("Any service".into(), true));
        assert_eq!(
            aud_label("did:web:a.test#x"),
            ("A service controlled by <b>a.test</b>".into(), true)
        );
        assert_eq!(
            aud_label("did:web:a.test"),
            ("did:web:a.test".into(), false)
        );
        assert_eq!(aud_label("<x>"), ("&lt;x&gt;".into(), false));
        let g = permission_groups(
            &scopes(&["atproto", "transition:generic"]),
            &BTreeMap::new(),
            false,
            None,
        );
        assert_eq!(g.groups[0].title, "Social app");
        assert!(!g.groups[0].description.contains("Bluesky"));
        let g = permission_groups(
            &scopes(&["atproto", "repo:app.bsky.feed.post"]),
            &BTreeMap::new(),
            false,
            None,
        );
        assert_eq!(
            g.groups[0].description,
            "Access specific parts of your Social app account"
        );
        let g = group(&[
            "atproto",
            "rpc:app.bsky.actor.getPreferences?aud=did:web:bsky.app#bsky_appview",
        ]);
        assert_eq!(g.groups[0].description, "Read your private preferences");
        assert!(g.groups[0].repo_table.is_none());
    }
}
