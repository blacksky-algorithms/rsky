//! Permission-set resolution for `include:<nsid>` OAuth scopes (proposal 0011).
//!
//! An `include:` names a permission set published as a `com.atproto.lexicon.schema`
//! record. Until it is fetched, the scope says nothing about what it confers, so
//! a session carrying only an `include:` has no grants this server can evaluate.
//! Resolving it is not a nicety: the ecosystem puts the real grants here. The
//! `app.bulleted.spaceAccess` set is what lets a Bulleted user read and write in
//! a space anchored on someone else, while the inline `space:` scope beside it
//! defaults `authority` to `self` and covers only the user's own spaces.
//!
//! Resolution follows the NSID's authority: `_lexicon.<authority>` TXT gives a
//! DID, whose document names the PDS holding the record.
//!
//! # Failure is a denial, not an opening
//!
//! A set that cannot be fetched contributes nothing. The alternative -- treating
//! an unreachable set as permissive -- would make a DNS outage into an
//! authorization bypass, which is the wrong direction to fail in. The cost is
//! that a network fault denies a user access to their own spaces, which is why
//! successes are cached for long enough to ride out a blip.

use hickory_resolver::config::{ResolverConfig, ResolverOpts};
use hickory_resolver::TokioAsyncResolver;
use rsky_syntax::nsid::Nsid;
use serde::Deserialize;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

const LEXICON_SUBDOMAIN: &str = "_lexicon";
const SCHEMA_COLLECTION: &str = "com.atproto.lexicon.schema";
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a resolved set is trusted. Permission sets change about as often as
/// an application's own lexicons, so this is long enough to make a transient
/// DNS or PDS fault invisible.
const SUCCESS_TTL: Duration = Duration::from_secs(3600);

/// How long a failure is remembered. Short, so a set that comes back is picked
/// up quickly, but non-zero so an unresolvable `include:` cannot make every
/// request from that session a fresh DNS lookup.
const FAILURE_TTL: Duration = Duration::from_secs(60);

/// One entry of a published permission set.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Permission {
    /// `space`, `repo`, `blob`, `rpc`, `identity`, or `account`.
    #[serde(default)]
    pub resource: String,
    #[serde(rename = "spaceType", default)]
    pub space_type: Option<String>,
    #[serde(default)]
    pub authority: Option<String>,
    #[serde(default)]
    pub skey: Option<String>,
    /// `repo` collections (0011 `RepoPermission.collection`).
    #[serde(default)]
    pub collection: Vec<String>,
    /// Actions for whichever resource names this: `repo`'s create/update/delete,
    /// or `account`'s read/manage.
    #[serde(default)]
    pub action: Vec<String>,
    #[serde(default, rename = "manage")]
    pub manage: Vec<String>,
    /// `rpc` methods (0011 `RpcPermission.lxm`).
    #[serde(default)]
    pub lxm: Vec<String>,
    /// `rpc`'s audience. `inherit_aud` is a coarser stand-in for "any
    /// audience" until this server can resolve the reference implementation's
    /// "the requesting PDS itself" semantics for `inheritAud`.
    #[serde(default)]
    pub aud: Option<String>,
    #[serde(default, rename = "inheritAud")]
    pub inherit_aud: bool,
    /// `blob` mime-type patterns (0011 `BlobPermission.accept`).
    #[serde(default)]
    pub accept: Vec<String>,
    /// `identity`'s attribute (`handle` or `*`) or `account`'s (`email`,
    /// `repo`, or `status`).
    #[serde(default)]
    pub attr: Option<String>,
}

impl Permission {
    /// The equivalent `space:` scope string, or `None` when this entry is not a
    /// space grant.
    ///
    /// Producing a scope string rather than a parsed grant means the resolved
    /// permissions are evaluated by exactly the same code as an inline
    /// `space:` scope, instead of a second implementation that could disagree
    /// with the first.
    #[must_use]
    pub fn to_space_scope(&self) -> Option<String> {
        if self.resource != "space" {
            return None;
        }
        let space_type = self.space_type.as_deref()?;
        let mut scope = format!("{}{space_type}", crate::space_scope::SPACE_SCOPE_PREFIX);
        let mut params: Vec<String> = Vec::new();
        if let Some(authority) = &self.authority {
            params.push(format!("authority={authority}"));
        }
        if let Some(skey) = &self.skey {
            params.push(format!("skey={skey}"));
        }
        for collection in &self.collection {
            params.push(format!("collection={collection}"));
        }
        for action in &self.action {
            params.push(format!("action={action}"));
        }
        for op in &self.manage {
            params.push(format!("manage={op}"));
        }
        if !params.is_empty() {
            scope.push('?');
            scope.push_str(&params.join("&"));
        }
        Some(scope)
    }

    /// The equivalent scope string for any of the five proposal-0011
    /// resource kinds this entry names, or `None` when it names none of them
    /// (an unrecognised `resource`, or one missing the field its kind
    /// requires).
    ///
    /// This is the general form of [`Self::to_space_scope`]: before it
    /// existed, a permission set's `repo`/`rpc`/`blob`/`identity`/`account`
    /// entries silently expanded into nothing (only `space` was handled),
    /// so a client that granted only a permission set -- the mechanism the
    /// proposal actually expects real clients to use -- ended up with an
    /// *unrestricted* session instead of the restricted one it asked for.
    /// Resolving every kind here, and feeding the result through the same
    /// parser an inline scope goes through, closes that gap.
    #[must_use]
    pub fn to_scope_string(&self) -> Option<String> {
        match self.resource.as_str() {
            "space" => self.to_space_scope(),
            "repo" => self.to_repo_scope(),
            "blob" => self.to_blob_scope(),
            "rpc" => self.to_rpc_scope(),
            "identity" => self.to_identity_scope(),
            "account" => self.to_account_scope(),
            _ => None,
        }
    }

    fn to_repo_scope(&self) -> Option<String> {
        if self.collection.is_empty() {
            return None;
        }
        let mut params: Vec<String> = self
            .collection
            .iter()
            .map(|c| format!("collection={c}"))
            .collect();
        params.extend(self.action.iter().map(|a| format!("action={a}")));
        Some(format!(
            "{}?{}",
            crate::oauth_scope::REPO_PREFIX,
            params.join("&")
        ))
    }

    fn to_blob_scope(&self) -> Option<String> {
        if self.accept.is_empty() {
            return None;
        }
        let params: Vec<String> = self.accept.iter().map(|a| format!("accept={a}")).collect();
        Some(format!(
            "{}?{}",
            crate::oauth_scope::BLOB_PREFIX,
            params.join("&")
        ))
    }

    fn to_rpc_scope(&self) -> Option<String> {
        if self.lxm.is_empty() {
            return None;
        }
        let aud = if self.inherit_aud {
            Some("*".to_string())
        } else {
            self.aud.clone()
        };
        let aud = aud?;
        let mut params: Vec<String> = self.lxm.iter().map(|l| format!("lxm={l}")).collect();
        params.push(format!("aud={aud}"));
        Some(format!(
            "{}?{}",
            crate::oauth_scope::RPC_PREFIX,
            params.join("&")
        ))
    }

    fn to_identity_scope(&self) -> Option<String> {
        let attr = self.attr.as_deref()?;
        Some(format!("{}{attr}", crate::oauth_scope::IDENTITY_PREFIX))
    }

    fn to_account_scope(&self) -> Option<String> {
        let attr = self.attr.as_deref()?;
        if self.action.is_empty() {
            return Some(format!("{}{attr}", crate::oauth_scope::ACCOUNT_PREFIX));
        }
        let params: Vec<String> = self.action.iter().map(|a| format!("action={a}")).collect();
        Some(format!(
            "{}{attr}?{}",
            crate::oauth_scope::ACCOUNT_PREFIX,
            params.join("&")
        ))
    }
}

#[derive(Debug, Deserialize)]
struct SchemaDef {
    #[serde(default, rename = "type")]
    def_type: String,
    #[serde(default)]
    permissions: Vec<Permission>,
}

#[derive(Debug, Deserialize)]
struct SchemaRecord {
    #[serde(default)]
    defs: HashMap<String, SchemaDef>,
}

#[derive(Debug, Deserialize)]
struct GetRecordOutput {
    value: SchemaRecord,
}

/// The scope strings a permission set confers -- across all five proposal
/// 0011 resource kinds, not just `space:` -- extracted from a fetched
/// `com.atproto.lexicon.schema` record.
fn resource_scopes_from_record(record: &SchemaRecord) -> Vec<String> {
    record
        .defs
        .values()
        .filter(|def| def.def_type == "permission-set")
        .flat_map(|def| def.permissions.iter())
        .filter_map(Permission::to_scope_string)
        .collect()
}

struct CacheEntry {
    scopes: Vec<String>,
    expires: Instant,
}

/// Resolves and caches permission sets.
#[derive(Default)]
pub struct PermissionSetResolver {
    cache: RwLock<HashMap<String, CacheEntry>>,
}

impl PermissionSetResolver {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The scope strings an `include:<nsid>` confers, across every
    /// proposal-0011 resource kind the set names. Empty when the set names no
    /// grants, and also when it could not be resolved -- the two are the same
    /// denial from a caller's point of view.
    pub async fn resolved_scopes(&self, nsid: &str) -> Vec<String> {
        if let Some(entry) = self.cache.read().await.get(nsid) {
            if entry.expires > Instant::now() {
                return entry.scopes.clone();
            }
        }
        let (scopes, ttl) = match self.fetch(nsid).await {
            Ok(scopes) => (scopes, SUCCESS_TTL),
            Err(error) => {
                tracing::debug!(%nsid, %error, "permission set unresolved; it confers nothing");
                (Vec::new(), FAILURE_TTL)
            }
        };
        self.cache.write().await.insert(
            nsid.to_string(),
            CacheEntry {
                scopes: scopes.clone(),
                expires: Instant::now() + ttl,
            },
        );
        scopes
    }

    async fn fetch(&self, nsid: &str) -> anyhow::Result<Vec<String>> {
        let parsed = Nsid::parse(nsid).map_err(|e| anyhow::anyhow!("invalid nsid: {e}"))?;
        let authority = parsed.authority();
        let did = resolve_lexicon_authority(&authority).await?;
        let endpoint = resolve_pds_endpoint(&did).await?;
        let client = reqwest::Client::builder().timeout(FETCH_TIMEOUT).build()?;
        let url = format!(
            "{}/xrpc/com.atproto.repo.getRecord",
            endpoint.trim_end_matches('/')
        );
        let response = client
            .get(&url)
            .query(&[
                ("repo", did.as_str()),
                ("collection", SCHEMA_COLLECTION),
                ("rkey", nsid),
            ])
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("{url} returned {}", response.status());
        }
        let output: GetRecordOutput = response.json().await?;
        Ok(resource_scopes_from_record(&output.value))
    }
}

/// `_lexicon.<authority>` TXT -> the DID publishing that authority's lexicons.
async fn resolve_lexicon_authority(authority: &str) -> anyhow::Result<String> {
    let resolver = TokioAsyncResolver::tokio(ResolverConfig::default(), ResolverOpts::default());
    let lookup = resolver
        .txt_lookup(format!("{LEXICON_SUBDOMAIN}.{authority}"))
        .await?;
    lookup
        .iter()
        .map(ToString::to_string)
        .find_map(|record| {
            record
                .trim()
                .strip_prefix("did=")
                .map(|did| did.trim().to_string())
        })
        .ok_or_else(|| anyhow::anyhow!("no did= TXT record at {LEXICON_SUBDOMAIN}.{authority}"))
}

async fn resolve_pds_endpoint(did: &str) -> anyhow::Result<String> {
    use rsky_identity::did::did_resolver::DidResolver;
    use rsky_identity::types::{DidResolverOpts, MemoryCache};
    use std::sync::Arc;

    let plc_url = rsky_common::env::env_str("PDS_DID_PLC_URL")
        .unwrap_or_else(|| "https://plc.directory".to_string());
    let resolver = DidResolver::new(DidResolverOpts {
        timeout: None,
        plc_url: Some(plc_url),
        did_cache: Arc::new(MemoryCache::new(None, None)),
    });
    let doc = resolver.ensure_resolve(&did.to_string(), None).await?;
    doc.service
        .as_deref()
        .unwrap_or_default()
        .iter()
        .find(|entry| entry.id.rsplit_once('#').map(|(_, f)| f) == Some("atproto_pds"))
        .map(|entry| entry.service_endpoint.clone())
        .ok_or_else(|| anyhow::anyhow!("no #atproto_pds service in the DID document for {did}"))
}

/// Rocket-managed handle for the resolver, so its cache is shared across
/// requests rather than rebuilt per session.
#[derive(Default)]
pub struct SharedPermissionSets {
    pub resolver: PermissionSetResolver,
}

/// Expand every `include:` in a session's granted scopes into the scope
/// strings it confers (`repo:`, `blob:`, `rpc:`, `identity:`, `account:`, or
/// `space:`), appended to the scopes as granted.
///
/// The result is fed to the same parser an inline scope of that kind goes
/// through, so a resolved permission behaves identically to one the client
/// wrote out. This is what makes a permission-set-only grant (no bare
/// `repo:`/`blob:`/etc. alongside the `include:`) actually restrict the
/// session instead of leaving `GrantedScopes` with nothing to enforce.
pub async fn expand_includes(resolver: &PermissionSetResolver, granted: &[String]) -> Vec<String> {
    let mut expanded = granted.to_vec();
    for scope in granted {
        if let Some(nsid) = scope.strip_prefix(crate::oauth_scope::INCLUDE_PREFIX) {
            expanded.extend(resolver.resolved_scopes(nsid).await);
        }
    }
    expanded
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real `app.bulleted.spaceAccess` record, fetched from
    /// `did:plc:geqwme5xqva5iasvlrwi4izj` on 2026-08-13. Vendored because the
    /// bug this file exists to fix was invisible to every round-trip test:
    /// they only ever asked our own code whether it agreed with itself.
    const BULLETED_SPACE_ACCESS: &str = r#"{
      "$type": "com.atproto.lexicon.schema",
      "lexicon": 1,
      "id": "app.bulleted.spaceAccess",
      "defs": {
        "main": {
          "type": "permission-set",
          "title": "Bulleted spaces",
          "detail": "Read the outlines shared in your Bulleted spaces, and write your own bullets in them.",
          "permissions": [
            {
              "type": "permission",
              "resource": "space",
              "spaceType": "app.bulleted.space",
              "authority": "*",
              "collection": [
                "app.bulleted.node",
                "app.bulleted.note",
                "app.bulleted.outline",
                "app.bulleted.mirror",
                "app.bulleted.comment",
                "app.bulleted.commentPolicy"
              ],
              "action": ["read", "create", "update", "delete"]
            }
          ]
        }
      }
    }"#;

    fn bulleted_scopes() -> Vec<String> {
        let record: SchemaRecord = serde_json::from_str(BULLETED_SPACE_ACCESS).unwrap();
        resource_scopes_from_record(&record)
    }

    #[test]
    fn a_published_set_becomes_scope_strings_the_normal_parser_accepts() {
        let scopes = bulleted_scopes();
        assert_eq!(scopes.len(), 1);
        let scope = &scopes[0];
        assert!(scope.starts_with("space:app.bulleted.space?"), "{scope}");
        assert!(scope.contains("authority=*"), "{scope}");
        assert!(scope.contains("collection=app.bulleted.node"), "{scope}");
        assert!(scope.contains("action=create"), "{scope}");
        // The whole point: it parses as a grant, not just as a string.
        crate::space_scope::SpaceScope::parse(scope).expect("resolved scope must parse");
    }

    #[test]
    fn entries_that_are_not_space_grants_are_skipped() {
        let repo_grant = Permission {
            resource: "repo".to_string(),
            collection: vec!["app.bulleted.node".to_string()],
            action: vec!["create".to_string()],
            ..Default::default()
        };
        assert!(repo_grant.to_space_scope().is_none());
        // A space entry with no space type names no spaces, so it grants none.
        let untyped = Permission {
            resource: "space".to_string(),
            space_type: None,
            ..repo_grant.clone()
        };
        assert!(untyped.to_space_scope().is_none());
    }

    #[test]
    fn a_bare_space_grant_needs_no_query_string() {
        let bare = Permission {
            resource: "space".to_string(),
            space_type: Some("app.bulleted.space".to_string()),
            ..Default::default()
        };
        assert_eq!(bare.to_space_scope().unwrap(), "space:app.bulleted.space");
    }

    #[test]
    fn manage_ops_survive_the_round_trip() {
        let managing = Permission {
            resource: "space".to_string(),
            space_type: Some("app.bulleted.space".to_string()),
            authority: Some("self".to_string()),
            skey: Some("main".to_string()),
            manage: vec!["create".to_string(), "delete".to_string()],
            ..Default::default()
        };
        let scope = managing.to_space_scope().unwrap();
        assert_eq!(
            scope,
            "space:app.bulleted.space?authority=self&skey=main&manage=create&manage=delete"
        );
        crate::space_scope::SpaceScope::parse(&scope).unwrap();
    }

    #[test]
    fn to_scope_string_expands_repo_blob_rpc_identity_and_account() {
        let repo = Permission {
            resource: "repo".to_string(),
            collection: vec!["app.bulleted.node".to_string()],
            action: vec!["create".to_string()],
            ..Default::default()
        };
        let scope = repo.to_scope_string().unwrap();
        assert!(scope.starts_with("repo:?"), "{scope}");
        assert!(scope.contains("collection=app.bulleted.node"), "{scope}");
        assert!(scope.contains("action=create"), "{scope}");
        assert!(crate::oauth_scope::GrantedScopes::parse(&[scope])
            .allows_repo("app.bulleted.node", crate::oauth_scope::RepoAction::Create));

        let blob = Permission {
            resource: "blob".to_string(),
            accept: vec!["image/*".to_string()],
            ..Default::default()
        };
        assert_eq!(blob.to_scope_string().unwrap(), "blob:?accept=image/*");

        let rpc = Permission {
            resource: "rpc".to_string(),
            lxm: vec!["com.example.method".to_string()],
            aud: Some("did:web:example.com".to_string()),
            ..Default::default()
        };
        let scope = rpc.to_scope_string().unwrap();
        assert!(scope.starts_with("rpc:?"), "{scope}");
        assert!(scope.contains("lxm=com.example.method"), "{scope}");
        assert!(scope.contains("aud=did:web:example.com"), "{scope}");

        let identity = Permission {
            resource: "identity".to_string(),
            attr: Some("handle".to_string()),
            ..Default::default()
        };
        assert_eq!(identity.to_scope_string().unwrap(), "identity:handle");

        let account = Permission {
            resource: "account".to_string(),
            attr: Some("email".to_string()),
            action: vec!["manage".to_string()],
            ..Default::default()
        };
        assert_eq!(
            account.to_scope_string().unwrap(),
            "account:email?action=manage"
        );

        // Missing the field its kind requires: no grant.
        assert!(Permission {
            resource: "rpc".to_string(),
            lxm: vec!["com.example.method".to_string()],
            ..Default::default()
        }
        .to_scope_string()
        .is_none());
        // An unrecognised resource confers nothing.
        assert!(Permission {
            resource: "unknown-future-resource".to_string(),
            ..Default::default()
        }
        .to_scope_string()
        .is_none());
    }

    /// The bug this whole file exists to fix: a permission set naming only a
    /// `repo:` grant (no `space:`) used to expand into nothing, so a session
    /// that granted only `include:<nsid>` ended up with no `repo:` scope at
    /// all. Expanding non-space entries is what gives such a session the
    /// grants it was actually issued.
    #[tokio::test]
    async fn a_permission_set_only_grant_restricts_rather_than_failing_open() {
        const REPO_ONLY_SET: &str = r#"{
          "$type": "com.atproto.lexicon.schema",
          "lexicon": 1,
          "id": "app.example.repoAccess",
          "defs": {
            "main": {
              "type": "permission-set",
              "permissions": [
                {
                  "type": "permission",
                  "resource": "repo",
                  "action": ["create", "update", "delete"],
                  "collection": ["app.bsky.feed.post"]
                }
              ]
            }
          }
        }"#;
        let record: SchemaRecord = serde_json::from_str(REPO_ONLY_SET).unwrap();
        let scopes = resource_scopes_from_record(&record);
        assert_eq!(scopes.len(), 1);

        // Simulate what `expand_includes` produces for a session that
        // granted only the permission set: `atproto` plus the resolved
        // `repo:` scope, no bare `repo:` of its own.
        let mut granted = vec!["atproto".to_string()];
        granted.extend(scopes);
        let granted_scopes = crate::oauth_scope::GrantedScopes::parse(&granted);
        assert!(granted_scopes
            .allows_repo("app.bsky.feed.post", crate::oauth_scope::RepoAction::Create));
        assert!(!granted_scopes
            .allows_repo("app.bsky.feed.like", crate::oauth_scope::RepoAction::Create));
    }

    #[tokio::test]
    async fn an_unresolvable_set_confers_nothing_and_is_not_retried_immediately() {
        let resolver = PermissionSetResolver::new();
        // `.invalid` is reserved by RFC 2606 and never resolves.
        let first = resolver.resolved_scopes("invalid.example.nothing").await;
        assert!(first.is_empty());
        // The failure is remembered, so the next call is a cache hit rather
        // than another DNS lookup.
        let cached = resolver.cache.read().await;
        assert!(cached.contains_key("invalid.example.nothing"));
    }

    /// The whole chain against the live network: `_lexicon.bulleted.app` TXT,
    /// the DID document, and the record itself.
    ///
    /// Ignored by default because it depends on DNS and someone else's server,
    /// and a test that fails when a third party has an outage is a test people
    /// learn to ignore. Run it with `--ignored` when touching resolution.
    #[tokio::test]
    #[ignore = "requires network and a third-party PDS"]
    async fn resolves_the_real_bulleted_permission_set() {
        let resolver = PermissionSetResolver::new();
        let scopes = resolver.resolved_scopes("app.bulleted.spaceAccess").await;
        assert_eq!(
            scopes,
            bulleted_scopes(),
            "the published set no longer matches the vendored fixture"
        );
    }

    #[tokio::test]
    async fn expansion_leaves_scopes_that_are_not_includes_alone() {
        let resolver = PermissionSetResolver::new();
        let granted = vec!["atproto".to_string(), "blob:image/*".to_string()];
        assert_eq!(expand_includes(&resolver, &granted).await, granted);
    }
}
