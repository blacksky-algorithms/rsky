use crate::did::plc_resolver::DidPlcResolver;
use crate::did::web_resolver::DidWebResolver;
use crate::errors::Error;
use crate::types::{CacheResult, DidCache, DidDocument, DidResolverOpts};
use anyhow::{bail, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;

#[derive(Clone, Debug)]
pub enum ResolverKind {
    Plc(DidPlcResolver),
    Web(DidWebResolver),
}

impl ResolverKind {
    pub async fn resolve_no_check(&self, did: String) -> Result<Option<Value>> {
        match self {
            Self::Plc(plc) => plc.resolve_no_check(did).await,
            Self::Web(web) => web.resolve_no_check(did).await,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DidResolver {
    pub cache: Option<DidCache>,
    pub methods: BTreeMap<String, ResolverKind>,
}

impl DidResolver {
    pub fn new(opts: DidResolverOpts) -> Self {
        let DidResolverOpts {
            timeout, plc_url, ..
        } = opts;
        let timeout = timeout.unwrap_or_else(|| Duration::new(3, 0));
        let plc_url = plc_url.unwrap_or_else(|| "https://plc.directory".to_string());

        let mut methods = BTreeMap::new();
        methods.insert(
            "plc".to_string(),
            ResolverKind::Plc(DidPlcResolver::new(plc_url, timeout.clone(), None)),
        );
        methods.insert(
            "web".to_string(),
            ResolverKind::Web(DidWebResolver::new(timeout, None)),
        );

        // do not pass cache to sub-methods, or we will be double caching
        Self {
            cache: Some(opts.did_cache),
            methods,
        }
    }

    pub async fn resolve_no_check(&self, did: String) -> Result<Option<Value>> {
        let split = did.split(":").collect::<Vec<&str>>();
        if split[0] != "did" {
            bail!(Error::PoorlyFormattedDidError(did))
        }
        match self.methods.get(split[1]) {
            None => bail!(Error::UnsupportedDidMethodError(did)),
            Some(method) => method.resolve_no_check(did).await,
        }
    }

    pub fn validate_did_doc(&self, did: String, val: Value) -> Result<DidDocument> {
        match serde_json::from_value::<DidDocument>(val.clone()) {
            Ok(doc) => {
                if doc.id != did {
                    bail!(Error::PoorlyFormattedDidDocumentError(val))
                }
                Ok(doc)
            }
            Err(err) => {
                eprintln!("Failed at parsing: {err}");
                bail!(Error::PoorlyFormattedDidDocumentError(val))
            }
        }
    }

    pub async fn resolve_no_cache(&self, did: &String) -> Result<Option<DidDocument>> {
        match self.resolve_no_check(did.clone()).await? {
            None => Ok(None),
            Some(got) => Ok(Some(self.validate_did_doc(did.clone(), got)?)),
        }
    }

    pub async fn refresh_cache(&mut self, did: String) -> Result<()> {
        let resolver = self.clone();
        match self.cache {
            None => Ok(()),
            Some(ref mut cache) => {
                cache
                    .refresh_cache(did.clone(), || resolver.resolve_no_cache(&did))
                    .await
            }
        }
    }

    pub async fn resolve(
        &mut self,
        did: String,
        force_refresh: Option<bool>,
    ) -> Result<Option<DidDocument>> {
        let from_cache: Option<CacheResult>;
        let force_refresh = force_refresh.unwrap_or(false);
        match self.cache {
            None => (),
            Some(ref cache) if !force_refresh => {
                from_cache = cache.check_cache(did.clone())?;
                match from_cache {
                    None => (),
                    Some(from_cache) if !from_cache.expired => {
                        if from_cache.stale {
                            self.refresh_cache(did).await?;
                        }
                        return Ok(Some(from_cache.doc));
                    }
                    _ => (),
                }
            }
            _ => (),
        }

        match self.resolve_no_cache(&did).await? {
            None => {
                if let Some(ref mut cache) = self.cache {
                    cache.clear_entry(did)?;
                }
                Ok(None)
            }
            Some(got) => {
                if let Some(ref mut cache) = self.cache {
                    cache.cache_did(did, got.clone()).await?;
                }
                Ok(Some(got))
            }
        }
    }

    pub async fn ensure_resolve(
        &mut self,
        did: &String,
        force_refresh: Option<bool>,
    ) -> Result<DidDocument> {
        let force_refresh = force_refresh.unwrap_or(false);
        match self.resolve(did.to_string(), Some(force_refresh)).await? {
            None => bail!(Error::DidNotFoundError(did.to_string())),
            Some(result) => Ok(result),
        }
    }
}
