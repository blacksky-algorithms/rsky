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
#[derive(Debug, Clone, Deserialize)]
pub struct Permission {
    /// `space`, `repo`, `blob`, `rpc`. Only `space` is meaningful here.
    #[serde(default)]
    pub resource: String,
    #[serde(rename = "spaceType", default)]
    pub space_type: Option<String>,
    #[serde(default)]
    pub authority: Option<String>,
    #[serde(default)]
    pub skey: Option<String>,
    #[serde(default)]
    pub collection: Vec<String>,
    #[serde(default)]
    pub action: Vec<String>,
    #[serde(default, rename = "manage")]
    pub manage: Vec<String>,
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

/// The `space:` scope strings a permission set confers, extracted from a
/// fetched `com.atproto.lexicon.schema` record.
fn space_scopes_from_record(record: &SchemaRecord) -> Vec<String> {
    record
        .defs
        .values()
        .filter(|def| def.def_type == "permission-set")
        .flat_map(|def| def.permissions.iter())
        .filter_map(Permission::to_space_scope)
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

    /// The `space:` scope strings an `include:<nsid>` confers. Empty when the
    /// set names no space grants, and also when it could not be resolved --
    /// the two are the same denial from a caller's point of view.
    pub async fn space_scopes(&self, nsid: &str) -> Vec<String> {
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
        Ok(space_scopes_from_record(&output.value))
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

/// Expand every `include:` in a session's granted scopes into the `space:`
/// scope strings it confers, appended to the scopes as granted.
///
/// The result is fed to the same parser an inline `space:` goes through, so a
/// resolved permission behaves identically to one the client wrote out.
pub async fn expand_includes(resolver: &PermissionSetResolver, granted: &[String]) -> Vec<String> {
    let mut expanded = granted.to_vec();
    for scope in granted {
        if let Some(nsid) = scope.strip_prefix(crate::oauth_scope::INCLUDE_PREFIX) {
            expanded.extend(resolver.space_scopes(nsid).await);
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
        space_scopes_from_record(&record)
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
            space_type: None,
            authority: None,
            skey: None,
            collection: vec!["app.bulleted.node".to_string()],
            action: vec!["create".to_string()],
            manage: Vec::new(),
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
            authority: None,
            skey: None,
            collection: Vec::new(),
            action: Vec::new(),
            manage: Vec::new(),
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
            collection: Vec::new(),
            action: Vec::new(),
            manage: vec!["create".to_string(), "delete".to_string()],
        };
        let scope = managing.to_space_scope().unwrap();
        assert_eq!(
            scope,
            "space:app.bulleted.space?authority=self&skey=main&manage=create&manage=delete"
        );
        crate::space_scope::SpaceScope::parse(&scope).unwrap();
    }

    #[tokio::test]
    async fn an_unresolvable_set_confers_nothing_and_is_not_retried_immediately() {
        let resolver = PermissionSetResolver::new();
        // `.invalid` is reserved by RFC 2606 and never resolves.
        let first = resolver.space_scopes("invalid.example.nothing").await;
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
        let scopes = resolver.space_scopes("app.bulleted.spaceAccess").await;
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
