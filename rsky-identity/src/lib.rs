extern crate url;

use crate::did::did_resolver::DidResolver;
use crate::handle::HandleResolver;
use crate::types::{DidCache, DidResolverOpts, HandleResolverOpts, IdentityResolverOpts};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct IdResolver {
    pub handle: HandleResolver,
    pub did: DidResolver,
}

impl IdResolver {
    pub fn new(opts: IdentityResolverOpts) -> Self {
        let IdentityResolverOpts {
            timeout,
            plc_url,
            did_cache,
            backup_nameservers,
        } = opts;
        let timeout = timeout.unwrap_or_else(|| Duration::from_millis(3000));
        let did_cache = did_cache.unwrap_or_else(|| DidCache {
            stale_ttl: Default::default(),
            max_ttl: Default::default(),
            cache: Default::default(),
        });

        Self {
            handle: HandleResolver::new(HandleResolverOpts {
                timeout: Some(timeout),
                backup_nameservers,
            }),
            did: DidResolver::new(DidResolverOpts {
                timeout: Some(timeout),
                plc_url,
                did_cache,
            }),
        }
    }
}

pub mod common;
pub mod did;
pub mod errors;
pub mod handle;
pub mod types;
