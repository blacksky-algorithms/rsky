//! Core permissioned-data record and commit types (proposal §Commit signature,
//! §Incremental sync).

use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;

/// A signed commit summarizing a permissioned repo
/// (`com.atproto.space.defs#signedCommit`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignedCommit {
    /// Commit format version, currently `1`.
    pub ver: u8,
    /// `sha256` of the LtHash state (32 bytes).
    pub hash: ByteBuf,
    /// Per-signature nonce (32 random bytes), fresh per reader.
    pub ikm: ByteBuf,
    /// `sign(ctx)` by the user's signing key.
    pub sig: ByteBuf,
    /// `HMAC-SHA256(HKDF-SHA256(ikm, ctx), hash)`.
    pub mac: ByteBuf,
    /// Commit revision (TID), also bound into `ctx`.
    pub rev: String,
}

/// A single entry in a repo's operation log (`listRepoOps`).
///
/// `cid` is `None` for a delete; `prev` is `None` for a create. Operations that
/// mutated atomically share a `rev`. When inlined, `value` carries the current
/// DAG-CBOR-encoded record bytes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoOp {
    pub rev: String,
    pub collection: String,
    pub rkey: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<ByteBuf>,
}

impl RepoOp {
    pub fn is_delete(&self) -> bool {
        self.cid.is_none()
    }
    pub fn is_create(&self) -> bool {
        self.prev.is_none()
    }
}

/// A member of the writer set returned by `listRepos`: an account that has
/// written at least one record into the space, with its current head.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoRef {
    pub did: String,
    pub rev: String,
    /// Hex-encoded current commit hash, if the authority tracks it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(cid: Option<&str>, prev: Option<&str>) -> RepoOp {
        RepoOp {
            rev: "3krev".to_string(),
            collection: "c.o.l".to_string(),
            rkey: "3ka".to_string(),
            cid: cid.map(str::to_string),
            prev: prev.map(str::to_string),
            value: None,
        }
    }

    #[test]
    fn op_kind_predicates() {
        assert!(op(None, Some("bafyOld")).is_delete());
        assert!(op(Some("bafyNew"), None).is_create());
        let update = op(Some("bafyNew"), Some("bafyOld"));
        assert!(!update.is_delete());
        assert!(!update.is_create());
    }

    #[test]
    fn wire_types_serde_roundtrip() {
        let commit = SignedCommit {
            ver: 1,
            hash: ByteBuf::from(vec![1u8; 32]),
            ikm: ByteBuf::from(vec![2u8; 32]),
            sig: ByteBuf::from(vec![3u8; 64]),
            mac: ByteBuf::from(vec![4u8; 32]),
            rev: "3krev".to_string(),
        };
        let json = serde_json::to_string(&commit).unwrap();
        assert_eq!(serde_json::from_str::<SignedCommit>(&json).unwrap(), commit);

        let repo = RepoRef {
            did: "did:plc:writer".to_string(),
            rev: "3krev".to_string(),
            hash: None,
        };
        let json = serde_json::to_string(&repo).unwrap();
        // Absent hash is omitted on the wire, not serialized as null.
        assert!(!json.contains("hash"));
        assert_eq!(serde_json::from_str::<RepoRef>(&json).unwrap(), repo);

        let o = op(Some("bafy"), None);
        let json = serde_json::to_string(&o).unwrap();
        assert_eq!(serde_json::from_str::<RepoOp>(&json).unwrap(), o);
    }
}
