use dasl::drisl;
use jacquard_common::BosStr;
use jacquard_common::types::cid::IpldCid;
use jacquard_repo::commit::Commit as JacqCommit;
use repo_stream::Commit as RSCommit;
use serde::Serialize;

use crate::identity::{DidError, SignatureError, SigningKey};
use crate::tid::TidParseError;
use crate::{DaslCid, Did, Tid};

#[derive(Debug, thiserror::Error)]
pub enum CommitConvertError {
    #[error("DID parse: {0}")]
    DidParse(#[from] DidError),
    #[error("Tid parse: {0}")]
    TidParse(#[from] TidParseError),
    #[error("Bad CID at {0}")]
    BadCid(&'static str),
}

/// a (slightly reduced) atproto commit object
///
/// we have commit types from jacquard and repo-stream which each use slightly
/// different newtypes for things like CIDs and Tids than what we actually use
/// in hubble-sync: so this is our internal type that we map those into.
///
/// for now it's not atproto-(de)serializeable. the typical way in is through
/// the jacquard/repo-stream types.
#[derive(Debug)]
pub struct CommitObject {
    /// repo identifier
    pub did: Did,
    /// mst root
    pub data: DaslCid,
    /// revision of the repo
    ///
    /// values too far in the future are invalid (not checked here)
    pub rev: Tid,
    /// usually unused
    ///
    /// in previous versions `prev` held the previous commit's CID; in v3 it's
    /// not needed. someeeeeee producers still set this (eg., bridgy)
    pub prev: Option<DaslCid>,
    /// cryptographic signature of this commit, as raw bytes
    pub sig: Vec<u8>,
}

impl CommitObject {
    pub fn data_ipld(&self) -> IpldCid {
        IpldCid::read_bytes(self.data.as_bytes()).expect("dasl cid to be valid")
    }
    /// verify `sig` over this commit's canonical unsigned encoding
    ///
    /// resolving the correct key for the repo is the caller's problem.
    pub fn verify_signature(&self, key: &SigningKey) -> Result<(), SignatureError> {
        let unsigned: UnsignedCommit = self.into();
        let signed_bytes = drisl::to_vec(&unsigned).expect("vec encode infallible");
        key.verify_bytes(&signed_bytes, &self.sig)
    }
    /// get encoded signed commit bytes
    pub fn to_drisl_bytes(&self) -> Vec<u8> {
        drisl::to_vec(&SignedCommit {
            did: self.did.as_str(),
            version: 3,
            data: self.data,
            rev: self.rev.to_string(),
            prev: self.prev,
            sig: &self.sig,
        })
        .expect("vec encode infallible")
    }
}

impl TryFrom<&RSCommit> for CommitObject {
    type Error = CommitConvertError;
    fn try_from(rsc: &RSCommit) -> Result<Self, Self::Error> {
        let did = rsc.did.parse()?;
        let data = DaslCid::from_bytes_raw(&rsc.data.to_bytes())
            .map_err(|_| CommitConvertError::BadCid("data"))?;
        let rev = rsc.rev.parse()?;
        let prev = rsc
            .prev
            .map(|t| DaslCid::from_bytes_raw(&t.to_bytes()))
            .transpose()
            .map_err(|_| CommitConvertError::BadCid("prev"))?;
        let sig = rsc.sig.to_vec();
        Ok(Self {
            did,
            data,
            rev,
            prev,
            sig,
        })
    }
}

impl<S: BosStr> TryFrom<JacqCommit<S>> for CommitObject {
    type Error = CommitConvertError;
    fn try_from(jac: JacqCommit<S>) -> Result<Self, Self::Error> {
        let did = Did::raw(jac.did.as_str());
        let data = DaslCid::from_bytes_raw(&jac.data.to_bytes())
            .map_err(|_| CommitConvertError::BadCid("data"))?;
        let rev = jac.rev.into();
        let prev = jac
            .prev
            .map(|t| DaslCid::from_bytes_raw(&t.to_bytes()))
            .transpose()
            .map_err(|_| CommitConvertError::BadCid("prev"))?;
        let sig = jac.sig.to_vec();
        Ok(Self {
            did,
            data,
            rev,
            prev,
            sig,
        })
    }
}

/// the commit representation that gets signed
///
/// `sig` is absent, gets dag-cbor (drisl)-encoded
#[derive(Debug, Serialize)]
pub struct UnsignedCommit<'a> {
    did: &'a str,
    version: u64, // always 3
    data: DaslCid,
    rev: String,           // tid isn't string, so not easy to borrow
    prev: Option<DaslCid>, // must be present (null when unset); some PDSes still set it in v3
}

impl<'a> From<&'a CommitObject> for UnsignedCommit<'a> {
    fn from(c: &'a CommitObject) -> Self {
        Self {
            did: c.did.as_str(),
            version: 3,
            data: c.data,
            rev: c.rev.to_string(),
            prev: c.prev,
        }
    }
}

/// full signed commit
#[derive(Serialize)]
struct SignedCommit<'a> {
    did: &'a str,
    version: u64, // always 3
    data: DaslCid,
    rev: String,
    prev: Option<DaslCid>, // null for most v3 repos, but some PDSes still set it
    #[serde(with = "serde_bytes")]
    sig: &'a [u8],
}
