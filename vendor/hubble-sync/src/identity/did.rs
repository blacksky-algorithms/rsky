//! DID newtype
//!
//! it's `Arc<str>` inside.
//!
//! has a `Borrow<str>` impl so `&str` can index into a `HashMap<Did, _>`

use std::borrow::Borrow;
use std::sync::Arc;

#[derive(Debug, Clone, thiserror::Error)]
pub enum DidError {
    #[error("unsupported DID method: {0:?}")]
    UnsupportedMethod(String),
    #[error("bad DID: {0:?}")]
    Bad(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DidMethod {
    Plc,
    Web,
}

impl DidMethod {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Plc => "plc",
            Self::Web => "web",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Did(Arc<str>);

impl Did {
    /// typed DID
    pub fn new(s: impl Into<Arc<str>>) -> Result<Self, DidError> {
        let s: Arc<str> = s.into();

        // all atproto DIDs methods must be ascii-only
        if !s.is_ascii() {
            return Err(DidError::Bad("non-ascii DID".to_string()));
        }
        if s.starts_with("did:plc:") {
            // basic check, not comprehensive
            if s.len() != 32 {
                return Err(DidError::Bad(format!(
                    "did:plc expected length=32, found {}: {s:?}",
                    s.len()
                )));
            }
            Ok(Self(s))
        } else if s.starts_with("did:web:") {
            Ok(Self(s)) // for now just accept anything
        } else if s.starts_with("did:") {
            Err(DidError::UnsupportedMethod(s.to_string()))
        } else {
            Err(DidError::Bad(s.to_string()))
        }
    }

    /// Unchecked DID wrapping
    ///
    /// caller must ensure that the DID is valid! you probably want `::new()`.
    pub fn raw(s: impl Into<Arc<str>>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// check if `self` and `other` are clones of the same thing, not just an
    /// equivalent DID inside.
    pub fn ptr_eq(&self, other: &Did) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub fn method(&self) -> DidMethod {
        if self.0.starts_with("did:plc:") {
            DidMethod::Plc
        } else if self.0.starts_with("did:web:") {
            DidMethod::Web
        } else {
            // unreachable unless some corrupt string got in (raw top suspect)
            panic!("unsupported DID method in Did: {self}");
        }
    }
}

impl AsRef<str> for Did {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for Did {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Did {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::str::FromStr for Did {
    type Err = DidError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<&str> for Did {
    type Error = DidError;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl From<&jacquard_common::types::string::Did> for Did {
    fn from(d: &jacquard_common::types::string::Did) -> Self {
        // TODO: we could probably get the inner smolstr and use its conversion
        // to Arc<str> instead of going through .as_str() but whatever
        Self(Arc::from(d.as_str()))
    }
}

impl From<&Did> for jacquard_common::types::string::Did {
    fn from(d: &Did) -> Self {
        jacquard_common::types::string::Did::raw(d.as_ref().into())
    }
}

/// did-as-str serialization helper
pub mod did_atproto {
    use crate::Did;
    use serde::{Deserialize, Deserializer, Serializer, de};
    use std::borrow::Cow;
    pub fn serialize<S: Serializer>(d: &Did, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(d.as_str())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Did, D::Error> {
        Cow::<str>::deserialize(de)?
            .parse()
            .map_err(de::Error::custom)
    }
}

/// did-as-str serialization helper
pub mod did_atproto_opt {
    use crate::Did;
    use serde::{Deserialize, Deserializer, Serializer, de};
    use std::borrow::Cow;
    pub fn serialize<S: Serializer>(did: &Option<Did>, ser: S) -> Result<S::Ok, S::Error> {
        match did {
            Some(d) => ser.serialize_some(d.as_str()),
            None => ser.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Option<Did>, D::Error> {
        Option::<Cow<str>>::deserialize(de)?
            .map(Did::new)
            .transpose()
            .map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn round_trips_as_str() {
        let d = Did::raw("did:plc:abc");
        assert_eq!(d.as_str(), "did:plc:abc");
        assert_eq!(d.to_string(), "did:plc:abc");
    }

    #[test]
    fn clone_shares_alloc() {
        let d = Did::raw("did:plc:abc");
        let e = d.clone();
        assert!(d.ptr_eq(&e));
    }

    #[test]
    fn independently_constructed_same_string_not_ptr_eq() {
        let a = Did::raw("did:plc:abc");
        let b = Did::raw("did:plc:abc");
        assert_eq!(a, b, "by-value equality holds");
        assert!(!a.ptr_eq(&b), "but they're separate allocations");
    }

    #[test]
    fn borrow_as_str_indexes_hashmap() {
        let mut m: HashMap<Did, i32> = HashMap::new();
        m.insert(Did::raw("did:plc:abc"), 42);
        // direct lookup with a &str via Borrow<str>
        assert_eq!(m.get("did:plc:abc"), Some(&42));
    }
}
