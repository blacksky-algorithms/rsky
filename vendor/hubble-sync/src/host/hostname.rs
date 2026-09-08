use super::Host;

use std::borrow::Borrow;
use std::fmt;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum HostnameError {
    #[error("failed parse: {0}: {1:?}")]
    Parse(String, String),
    #[error("not a bare hostname: {0:?}")]
    NotBare(String),
    #[error("not a hostname: {0:?}")]
    MissingHost(String),
}

/// newtyped hostname wrapper
///
/// you should get one through the Host interner
///
/// and you should pass around its owning `Host`, not this directly
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Hostname(pub(super) Arc<str>);

impl Hostname {
    pub fn as_str(&self) -> &str {
        self.0.as_ref()
    }

    pub fn is_bsky(&self) -> bool {
        self.0.ends_with(".host.bsky.network")
    }

    #[cfg(test)]
    pub fn new(s: &str) -> Self {
        Self(Arc::from(s))
    }
    #[cfg(test)]
    pub(crate) fn arc(&self) -> &Arc<str> {
        &self.0
    }
}

impl Borrow<str> for Hostname {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for Hostname {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub(super) fn normalize_hostname(name: &str) -> Result<String, HostnameError> {
    use reqwest::Url;
    let presentable = || name[..name.floor_char_boundary(80)].to_string();

    let url = Url::parse(&format!("https://{name}"))
        .map_err(|e| HostnameError::Parse(e.to_string(), presentable()))?;

    if url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(HostnameError::NotBare(presentable()));
    }

    let host = url
        .host_str()
        .ok_or_else(|| HostnameError::MissingHost(presentable()))?;

    let undotted = host.trim_end_matches('.');
    if undotted.is_empty() {
        // somehow the hostname was *only* a dot
        return Err(HostnameError::MissingHost(presentable()));
    }

    Ok(undotted.to_string())
}

pub fn serialize_name<S: serde::Serializer>(h: &Host, ser: S) -> Result<S::Ok, S::Error> {
    ser.serialize_str(h.name().as_str())
}
