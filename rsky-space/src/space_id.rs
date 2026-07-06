//! Space and permissioned-record addressing (proposal §Addressing).
//!
//! ```text
//! Space:  at://{spaceDid}/space/{spaceType}/{skey}
//! Record: at://{spaceDid}/space/{spaceType}/{skey}/{authorDid}/{collection}/{rkey}
//! ```
//!
//! The literal `space` marker sits where a collection NSID would appear in a
//! public at-uri. The two are never ambiguous: a public collection NSID always
//! contains at least two `.`s, whereas `space` contains none.

use crate::error::{Result, SpaceError};

/// The fixed marker segment identifying a permissioned-space URI.
pub const SPACE_MARKER: &str = "space";

/// A space identity: `(authority, type, skey)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceId {
    pub authority: String,
    pub space_type: String,
    pub skey: String,
}

impl SpaceId {
    pub fn new(
        authority: impl Into<String>,
        space_type: impl Into<String>,
        skey: impl Into<String>,
    ) -> Self {
        Self {
            authority: authority.into(),
            space_type: space_type.into(),
            skey: skey.into(),
        }
    }

    /// `at://{authority}/space/{type}/{skey}`
    pub fn uri(&self) -> String {
        format!(
            "at://{}/{}/{}/{}",
            self.authority, SPACE_MARKER, self.space_type, self.skey
        )
    }

    /// Parse a space URI. Rejects public at-uris (no `space` marker).
    pub fn parse(uri: &str) -> Result<Self> {
        let rest = uri
            .strip_prefix("at://")
            .ok_or_else(|| SpaceError::InvalidSpaceUri(uri.to_string()))?;
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() != 4 || parts[1] != SPACE_MARKER {
            return Err(SpaceError::InvalidSpaceUri(uri.to_string()));
        }
        Ok(Self::new(parts[0], parts[2], parts[3]))
    }

    /// Build a permissioned-record URI within this space.
    pub fn record_uri(&self, author_did: &str, collection: &str, rkey: &str) -> String {
        format!("{}/{}/{}/{}", self.uri(), author_did, collection, rkey)
    }
}

/// True if `uri` is a permissioned-space URI (has the `space` marker segment).
pub fn is_space_uri(uri: &str) -> bool {
    uri.strip_prefix("at://")
        .and_then(|rest| rest.split('/').nth(1))
        .map(|seg| seg == SPACE_MARKER)
        .unwrap_or(false)
}

/// A fully-qualified permissioned record reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordId {
    pub space: SpaceId,
    pub author: String,
    pub collection: String,
    pub rkey: String,
}

impl RecordId {
    pub fn parse(uri: &str) -> Result<Self> {
        let rest = uri
            .strip_prefix("at://")
            .ok_or_else(|| SpaceError::InvalidRecordUri(uri.to_string()))?;
        let p: Vec<&str> = rest.split('/').collect();
        if p.len() != 7 || p[1] != SPACE_MARKER {
            return Err(SpaceError::InvalidRecordUri(uri.to_string()));
        }
        Ok(Self {
            space: SpaceId::new(p[0], p[2], p[3]),
            author: p[4].to_string(),
            collection: p[5].to_string(),
            rkey: p[6].to_string(),
        })
    }

    pub fn uri(&self) -> String {
        self.space
            .record_uri(&self.author, &self.collection, &self.rkey)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTH: &str = "did:plc:spaceauthority";
    const AUTHOR: &str = "did:plc:author";

    #[test]
    fn space_uri_roundtrip() {
        let s = SpaceId::new(AUTH, "community.blacksky.feed", "main");
        let uri = s.uri();
        assert_eq!(
            uri,
            format!("at://{AUTH}/space/community.blacksky.feed/main")
        );
        assert_eq!(SpaceId::parse(&uri).unwrap(), s);
        assert!(is_space_uri(&uri));
    }

    #[test]
    fn record_uri_roundtrip() {
        let s = SpaceId::new(AUTH, "community.blacksky.feed", "main");
        let uri = s.record_uri(AUTHOR, "community.blacksky.feed.post", "3kabc");
        let rid = RecordId::parse(&uri).unwrap();
        assert_eq!(rid.author, AUTHOR);
        assert_eq!(rid.collection, "community.blacksky.feed.post");
        assert_eq!(rid.rkey, "3kabc");
        assert_eq!(rid.uri(), uri);
    }

    #[test]
    fn rejects_public_aturi() {
        let public = format!("at://{AUTHOR}/app.bsky.feed.post/3kabc");
        assert!(!is_space_uri(&public));
        assert!(SpaceId::parse(&public).is_err());
        assert!(RecordId::parse(&public).is_err());
    }
}
