//! `space:` OAuth scope grammar (permissioned-data proposal §OAuth scopes).
//!
//! ```text
//! space:<spaceType>[?authority=<did>][&skey=<skey>][&collection=<nsid>...]
//!                  [&action=<action>...][&manage=<op>...]
//! ```
//!
//! This module is pure: parsing and matching only. Wiring scopes to sessions
//! is the A7 OAuth track's concern; see `crate::space_auth` for the seam.
//!
//! Collection-default note: the spec defaults an omitted `collection` to the
//! space type declaration's `collections` list, resolved when the grant is
//! evaluated. Space-type declarations are not resolvable here yet, so an
//! omitted `collection` on a **concrete** space type is treated as
//! declared-collections semantics with an allow-all interpretation. A
//! wildcard space type has no declaration to draw from, so its default stays
//! empty and confers no write targets, per spec.

use std::fmt;

pub const SPACE_SCOPE_PREFIX: &str = "space:";
pub const MAX_SKEY_LEN: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceAction {
    ReadSelf,
    Read,
    Create,
    Update,
    Delete,
}

impl SpaceAction {
    fn parse(s: &str) -> Result<Self, ScopeParseError> {
        match s {
            "read_self" => Ok(SpaceAction::ReadSelf),
            "read" => Ok(SpaceAction::Read),
            "create" => Ok(SpaceAction::Create),
            "update" => Ok(SpaceAction::Update),
            "delete" => Ok(SpaceAction::Delete),
            _ => Err(ScopeParseError(format!("invalid action `{s}`"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManageOp {
    Create,
    Update,
    Delete,
}

impl ManageOp {
    fn parse(s: &str) -> Result<Self, ScopeParseError> {
        match s {
            "create" => Ok(ManageOp::Create),
            "update" => Ok(ManageOp::Update),
            "delete" => Ok(ManageOp::Delete),
            _ => Err(ScopeParseError(format!("invalid manage op `{s}`"))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeParseError(pub String);

impl fmt::Display for ScopeParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid space scope: {}", self.0)
    }
}

impl std::error::Error for ScopeParseError {}

/// A parsed `space:` scope grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceScope {
    /// Space-type NSID or `*`.
    pub space_type: String,
    /// Authority DID, `self`, or `*`. Defaults to `self`.
    pub authority: String,
    /// Space key or `*`. Defaults to `*`.
    pub skey: String,
    /// `None` means the space type declaration's `collections` default.
    pub collections: Option<Vec<String>>,
    /// `None` means the default set: read, create, update, delete.
    pub actions: Option<Vec<SpaceAction>>,
    /// Empty by default: a record-access grant confers no admin capability.
    pub manage: Vec<ManageOp>,
}

fn valid_nsid(s: &str) -> bool {
    // an NSID has at least two dots and non-empty segments
    s.matches('.').count() >= 2 && s.split('.').all(|seg| !seg.is_empty())
}

impl SpaceScope {
    pub fn parse(scope: &str) -> Result<Self, ScopeParseError> {
        let rest = scope
            .strip_prefix(SPACE_SCOPE_PREFIX)
            .ok_or_else(|| ScopeParseError(format!("missing `{SPACE_SCOPE_PREFIX}` prefix")))?;
        let (space_type, query) = match rest.split_once('?') {
            Some((st, q)) => (st, Some(q)),
            None => (rest, None),
        };
        if space_type != "*" && !valid_nsid(space_type) {
            return Err(ScopeParseError(format!(
                "space type `{space_type}` is not an NSID or `*`"
            )));
        }
        let mut authority: Option<String> = None;
        let mut skey: Option<String> = None;
        let mut collections: Vec<String> = Vec::new();
        let mut actions: Vec<SpaceAction> = Vec::new();
        let mut manage: Vec<ManageOp> = Vec::new();
        if let Some(query) = query {
            for pair in query.split('&') {
                let (key, value) = pair
                    .split_once('=')
                    .ok_or_else(|| ScopeParseError(format!("malformed parameter `{pair}`")))?;
                if value.is_empty() {
                    return Err(ScopeParseError(format!("empty value for `{key}`")));
                }
                match key {
                    "authority" => {
                        if authority.is_some() {
                            return Err(ScopeParseError("duplicate authority".into()));
                        }
                        if value != "self" && value != "*" && !value.starts_with("did:") {
                            return Err(ScopeParseError(format!(
                                "authority `{value}` is not a DID, `self`, or `*`"
                            )));
                        }
                        authority = Some(value.to_string());
                    }
                    "skey" => {
                        if skey.is_some() {
                            return Err(ScopeParseError("duplicate skey".into()));
                        }
                        if value.len() > MAX_SKEY_LEN {
                            return Err(ScopeParseError("skey too long".into()));
                        }
                        skey = Some(value.to_string());
                    }
                    "collection" => {
                        if value != "*" && !valid_nsid(value) {
                            return Err(ScopeParseError(format!(
                                "collection `{value}` is not an NSID or `*`"
                            )));
                        }
                        collections.push(value.to_string());
                    }
                    "action" => actions.push(SpaceAction::parse(value)?),
                    "manage" => manage.push(ManageOp::parse(value)?),
                    _ => return Err(ScopeParseError(format!("unknown parameter `{key}`"))),
                }
            }
        }
        Ok(SpaceScope {
            space_type: space_type.to_string(),
            authority: authority.unwrap_or_else(|| "self".to_string()),
            skey: skey.unwrap_or_else(|| "*".to_string()),
            collections: if collections.is_empty() {
                None
            } else {
                Some(collections)
            },
            actions: if actions.is_empty() {
                None
            } else {
                Some(actions)
            },
            manage,
        })
    }

    /// Whether this grant's `(authority, spaceType, skey)` selector covers the
    /// requested space. `self` resolves to the granting session's DID.
    pub fn covers_space(
        &self,
        session_did: &str,
        authority: &str,
        space_type: &str,
        skey: &str,
    ) -> bool {
        let authority_ok = match self.authority.as_str() {
            "*" => true,
            "self" => authority == session_did,
            grant => grant == authority,
        };
        authority_ok
            && (self.space_type == "*" || self.space_type == space_type)
            && (self.skey == "*" || self.skey == skey)
    }

    fn has_action(&self, action: SpaceAction) -> bool {
        match &self.actions {
            // omitted action grants read/create/update/delete (read implies read_self)
            None => true,
            Some(actions) => match action {
                SpaceAction::ReadSelf => {
                    actions.contains(&SpaceAction::ReadSelf) || actions.contains(&SpaceAction::Read)
                }
                other => actions.contains(&other),
            },
        }
    }

    fn permits_collection(&self, collection: &str) -> bool {
        match &self.collections {
            Some(collections) => collections.iter().any(|c| c == "*" || c == collection),
            // declared-collections default; see the module note. Wildcard
            // space types have no declaration, so the default is empty.
            None => self.space_type != "*",
        }
    }

    /// Whole-space read (covers every repo, ignores `collection`).
    pub fn permits_read(&self) -> bool {
        self.has_action(SpaceAction::Read)
    }

    /// Read of the holder's own repo, constrained by `collection` when the
    /// grant is `read_self`-only. A `read` grant is not collection-constrained.
    pub fn permits_read_self(&self, collection: Option<&str>) -> bool {
        if self.permits_read() {
            return true;
        }
        if !self.has_action(SpaceAction::ReadSelf) {
            return false;
        }
        match collection {
            Some(collection) => self.permits_collection(collection),
            None => true,
        }
    }

    /// A create/update/delete of a specific record, constrained by `collection`.
    pub fn permits_record_write(&self, action: SpaceAction, collection: &str) -> bool {
        matches!(
            action,
            SpaceAction::Create | SpaceAction::Update | SpaceAction::Delete
        ) && self.has_action(action)
            && self.permits_collection(collection)
    }

    /// A space-management operation. Ignores `collection`; omitted by default.
    pub fn permits_manage(&self, op: ManageOp) -> bool {
        self.manage.contains(&op)
    }
}

/// A concrete authorization request evaluated against a set of grants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceRequest {
    /// Whole-space read (any repo in the space); confers `getDelegationToken`.
    Read,
    /// Read of the requester's own repo, optionally narrowed to a collection.
    ReadSelf { collection: Option<String> },
    /// A record mutation.
    Write {
        action: SpaceAction,
        collection: String,
    },
    /// A space-management operation.
    Manage(ManageOp),
}

/// Protocol-level default: a session with no `space:` grants at all is denied.
pub fn authorize(
    scopes: &[SpaceScope],
    session_did: &str,
    authority: &str,
    space_type: &str,
    skey: &str,
    request: &SpaceRequest,
) -> bool {
    scopes.iter().any(|scope| {
        scope.covers_space(session_did, authority, space_type, skey)
            && match request {
                SpaceRequest::Read => scope.permits_read(),
                SpaceRequest::ReadSelf { collection } => {
                    scope.permits_read_self(collection.as_deref())
                }
                SpaceRequest::Write { action, collection } => {
                    scope.permits_record_write(*action, collection)
                }
                SpaceRequest::Manage(op) => scope.permits_manage(*op),
            }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SELF_DID: &str = "did:plc:granter";
    const OTHER_AUTH: &str = "did:plc:abc123";
    const FORUM: &str = "com.atmoboards.forum";
    const THREAD: &str = "com.atmoboards.thread";
    const REPLY: &str = "com.atmoboards.reply";

    fn scope(s: &str) -> SpaceScope {
        SpaceScope::parse(s).unwrap()
    }

    fn allowed(s: &str, authority: &str, request: &SpaceRequest) -> bool {
        authorize(&[scope(s)], SELF_DID, authority, FORUM, "default", request)
    }

    fn write(action: SpaceAction, collection: &str) -> SpaceRequest {
        SpaceRequest::Write {
            action,
            collection: collection.to_string(),
        }
    }

    #[test]
    fn parse_defaults() {
        let s = scope("space:com.example.bookmarks");
        assert_eq!(s.space_type, "com.example.bookmarks");
        assert_eq!(s.authority, "self");
        assert_eq!(s.skey, "*");
        assert!(s.collections.is_none());
        assert!(s.actions.is_none());
        assert!(s.manage.is_empty());
    }

    #[test]
    fn parse_full_form() {
        let s = scope(
            "space:com.atmoboards.forum?authority=did:plc:abc123&skey=default\
             &collection=com.atmoboards.thread&action=create&action=update&manage=delete",
        );
        assert_eq!(s.authority, OTHER_AUTH);
        assert_eq!(s.skey, "default");
        assert_eq!(s.collections, Some(vec![THREAD.to_string()]));
        assert_eq!(
            s.actions,
            Some(vec![SpaceAction::Create, SpaceAction::Update])
        );
        assert_eq!(s.manage, vec![ManageOp::Delete]);
    }

    #[test]
    fn parse_rejects_malformed_scopes() {
        for bad in [
            "repo:com.example.thing",                                // wrong prefix
            "space:notannsid",                                       // not an NSID
            "space:com.example.forum?authority=",                    // empty value
            "space:com.example.forum?authority",                     // no '='
            "space:com.example.forum?authority=nota-did",            // bad authority
            "space:com.example.forum?authority=self&authority=self", // duplicate
            "space:com.example.forum?skey=a&skey=b",                 // duplicate
            "space:com.example.forum?collection=nodots",             // bad collection
            "space:com.example.forum?action=admin",                  // bad action
            "space:com.example.forum?manage=read",                   // bad manage op
            "space:com.example.forum?unknown=1",                     // unknown param
        ] {
            assert!(SpaceScope::parse(bad).is_err(), "{bad} should fail");
        }
        let long_skey = format!("space:com.example.forum?skey={}", "a".repeat(513));
        assert!(SpaceScope::parse(&long_skey).is_err());
        let ok_skey = format!("space:com.example.forum?skey={}", "a".repeat(512));
        assert!(SpaceScope::parse(&ok_skey).is_ok());
    }

    // §Examples: `space:com.example.bookmarks`
    #[test]
    fn bare_scope_covers_own_spaces_with_full_access() {
        let s = "space:com.atmoboards.forum";
        // authority defaults to self
        assert!(allowed(s, SELF_DID, &SpaceRequest::Read));
        assert!(!allowed(s, OTHER_AUTH, &SpaceRequest::Read));
        // write access to declared collections (allow-all interim semantics)
        assert!(allowed(s, SELF_DID, &write(SpaceAction::Create, THREAD)));
        assert!(allowed(s, SELF_DID, &write(SpaceAction::Delete, REPLY)));
        // no manage capability by default
        assert!(!allowed(
            s,
            SELF_DID,
            &SpaceRequest::Manage(ManageOp::Update)
        ));
    }

    // §Examples: `space:com.atmoboards.forum?authority=*`
    #[test]
    fn wildcard_authority_reaches_shared_spaces() {
        let s = "space:com.atmoboards.forum?authority=*";
        assert!(allowed(s, OTHER_AUTH, &SpaceRequest::Read));
        assert!(allowed(s, SELF_DID, &SpaceRequest::Read));
        assert!(allowed(s, OTHER_AUTH, &write(SpaceAction::Create, THREAD)));
        // a different space type is not covered
        assert!(!authorize(
            &[scope(s)],
            SELF_DID,
            OTHER_AUTH,
            "com.example.other",
            "default",
            &SpaceRequest::Read
        ));
    }

    // §Examples: `space:com.atmoboards.forum?authority=*&action=read`
    #[test]
    fn read_only_grant_denies_writes_and_needs_no_collection() {
        let s = "space:com.atmoboards.forum?authority=*&action=read";
        assert!(allowed(s, OTHER_AUTH, &SpaceRequest::Read));
        assert!(allowed(
            s,
            OTHER_AUTH,
            &SpaceRequest::ReadSelf {
                collection: Some(THREAD.to_string())
            }
        ));
        assert!(!allowed(s, OTHER_AUTH, &write(SpaceAction::Create, THREAD)));
        assert!(!allowed(s, OTHER_AUTH, &write(SpaceAction::Delete, THREAD)));
    }

    // §Examples: `space:com.atmoboards.forum?authority=*&action=read_self&collection=*`
    #[test]
    fn read_self_grant_is_own_repo_only() {
        let s = "space:com.atmoboards.forum?authority=*&action=read_self&collection=*";
        assert!(!allowed(s, OTHER_AUTH, &SpaceRequest::Read));
        assert!(allowed(
            s,
            OTHER_AUTH,
            &SpaceRequest::ReadSelf {
                collection: Some("com.arbitrary.other.type".to_string())
            }
        ));
        assert!(allowed(
            s,
            OTHER_AUTH,
            &SpaceRequest::ReadSelf { collection: None }
        ));
        assert!(!allowed(s, OTHER_AUTH, &write(SpaceAction::Create, THREAD)));
    }

    // read_self without collection=* is constrained by the declared default
    #[test]
    fn read_self_collection_constraint() {
        let s = "space:com.atmoboards.forum?authority=*&action=read_self&collection=com.atmoboards.thread";
        assert!(allowed(
            s,
            OTHER_AUTH,
            &SpaceRequest::ReadSelf {
                collection: Some(THREAD.to_string())
            }
        ));
        assert!(!allowed(
            s,
            OTHER_AUTH,
            &SpaceRequest::ReadSelf {
                collection: Some(REPLY.to_string())
            }
        ));
        // a read grant satisfies read_self regardless of collection
        let read = "space:com.atmoboards.forum?authority=*&action=read";
        assert!(allowed(
            read,
            OTHER_AUTH,
            &SpaceRequest::ReadSelf {
                collection: Some(REPLY.to_string())
            }
        ));
    }

    // §Examples: `space:com.atmoboards.forum?authority=*&collection=*`
    #[test]
    fn wildcard_collection_widens_writes() {
        let s = "space:com.atmoboards.forum?authority=*&collection=*";
        assert!(allowed(s, OTHER_AUTH, &SpaceRequest::Read));
        assert!(allowed(
            s,
            OTHER_AUTH,
            &write(SpaceAction::Create, "com.arbitrary.other.type")
        ));
    }

    // §Examples: authority+skey+collection+action pinned grant
    #[test]
    fn pinned_grant_matches_exactly() {
        let s = "space:com.atmoboards.forum?authority=did:plc:abc123&skey=default\
                 &collection=com.atmoboards.thread&action=create&action=update";
        assert!(allowed(s, OTHER_AUTH, &write(SpaceAction::Create, THREAD)));
        assert!(allowed(s, OTHER_AUTH, &write(SpaceAction::Update, THREAD)));
        assert!(!allowed(s, OTHER_AUTH, &write(SpaceAction::Delete, THREAD)));
        assert!(!allowed(s, OTHER_AUTH, &write(SpaceAction::Create, REPLY)));
        // read was not granted
        assert!(!allowed(s, OTHER_AUTH, &SpaceRequest::Read));
        // wrong skey
        assert!(!authorize(
            &[scope(s)],
            SELF_DID,
            OTHER_AUTH,
            FORUM,
            "other-skey",
            &write(SpaceAction::Create, THREAD)
        ));
        // wrong authority
        assert!(!authorize(
            &[scope(s)],
            SELF_DID,
            "did:plc:someoneelse",
            FORUM,
            "default",
            &write(SpaceAction::Create, THREAD)
        ));
    }

    // §Examples: admin grants
    #[test]
    fn manage_grants() {
        // read_self + manage, no record writes
        let s =
            "space:com.atmoboards.forum?authority=*&action=read_self&manage=update&manage=delete";
        assert!(allowed(
            s,
            OTHER_AUTH,
            &SpaceRequest::Manage(ManageOp::Update)
        ));
        assert!(allowed(
            s,
            OTHER_AUTH,
            &SpaceRequest::Manage(ManageOp::Delete)
        ));
        assert!(!allowed(
            s,
            OTHER_AUTH,
            &SpaceRequest::Manage(ManageOp::Create)
        ));
        assert!(!allowed(s, OTHER_AUTH, &write(SpaceAction::Create, THREAD)));
        assert!(!allowed(s, OTHER_AUTH, &SpaceRequest::Read));

        // manage alongside default full record access
        let s = "space:com.atmoboards.forum?authority=*&manage=update&manage=delete";
        assert!(allowed(
            s,
            OTHER_AUTH,
            &SpaceRequest::Manage(ManageOp::Update)
        ));
        assert!(allowed(s, OTHER_AUTH, &write(SpaceAction::Create, THREAD)));
        assert!(allowed(s, OTHER_AUTH, &SpaceRequest::Read));

        // manage=create is typically granted with skey=*
        let s = "space:com.atmoboards.forum?manage=create";
        assert!(allowed(
            s,
            SELF_DID,
            &SpaceRequest::Manage(ManageOp::Create)
        ));
    }

    // §Examples: `space:*?authority=did:plc:abc123`
    #[test]
    fn wildcard_space_type_reads_but_confers_no_default_writes() {
        let s = "space:*?authority=did:plc:abc123";
        assert!(allowed(s, OTHER_AUTH, &SpaceRequest::Read));
        assert!(authorize(
            &[scope(s)],
            SELF_DID,
            OTHER_AUTH,
            "com.any.type",
            "any",
            &SpaceRequest::Read
        ));
        // no declaration to draw a collection default from: no write targets
        assert!(!allowed(s, OTHER_AUTH, &write(SpaceAction::Create, THREAD)));
        // unless a collection is provided explicitly
        let s = "space:*?authority=did:plc:abc123&collection=*";
        assert!(allowed(s, OTHER_AUTH, &write(SpaceAction::Create, THREAD)));
    }

    #[test]
    fn empty_scope_set_denies() {
        assert!(!authorize(
            &[],
            SELF_DID,
            SELF_DID,
            FORUM,
            "default",
            &SpaceRequest::Read
        ));
    }

    #[test]
    fn any_grant_in_the_set_suffices() {
        let scopes = vec![
            scope("space:com.other.thing"),
            scope("space:com.atmoboards.forum?authority=*&action=read"),
        ];
        assert!(authorize(
            &scopes,
            SELF_DID,
            OTHER_AUTH,
            FORUM,
            "default",
            &SpaceRequest::Read
        ));
        assert!(!authorize(
            &scopes,
            SELF_DID,
            OTHER_AUTH,
            FORUM,
            "default",
            &write(SpaceAction::Create, THREAD)
        ));
    }

    #[test]
    fn parse_error_displays() {
        let err = SpaceScope::parse("nope").unwrap_err();
        assert!(err.to_string().contains("invalid space scope"));
    }
}
