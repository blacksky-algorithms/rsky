use crate::repo::blob_refs::BlobRef;
use crate::repo::block_map::BlockMap;
use crate::repo::cid_set::CidSet;
use crate::repo::mst::CidAndBytes;
use crate::storage::Ipld;
use libipld::Cid;
use std::collections::BTreeMap;

// Repo nodes
// ---------------

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct UnsignedCommit {
    pub did: String,
    pub version: u8, // Should be 3
    pub data: Cid,
    pub rev: String,
    // `prev` added for backwards compatibility with v2, no requirement of keeping around history
    pub prev: Option<Cid>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Commit {
    pub did: String,
    pub version: u8, // Should be 3
    pub data: Cid,
    pub rev: String,
    pub prev: Option<Cid>,
    pub sig: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct LegacyV2Commit {
    pub did: String,
    pub version: u8, // Should be 2
    pub data: Cid,
    pub rev: Option<String>,
    pub prev: Option<Cid>,
    pub sig: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum VersionedCommit {
    Commit(Commit),
    LegacyV2Commit(LegacyV2Commit),
}

impl VersionedCommit {
    pub fn data(&self) -> Cid {
        match self {
            VersionedCommit::Commit(c) => c.data,
            VersionedCommit::LegacyV2Commit(c) => c.data,
        }
    }

    pub fn did(&self) -> &String {
        match self {
            VersionedCommit::Commit(c) => &c.did,
            VersionedCommit::LegacyV2Commit(c) => &c.did,
        }
    }

    pub fn version(&self) -> u8 {
        match self {
            VersionedCommit::Commit(c) => c.version,
            VersionedCommit::LegacyV2Commit(c) => c.version,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum Lex {
    Ipld(Ipld),
    Blob(BlobRef),
    List(Vec<Lex>),
    Map(BTreeMap<String, Lex>),
}

// Repo Operations
// ---------------

pub type RepoRecord = BTreeMap<String, Lex>;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum WriteOpAction {
    Create,
    Update,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RecordCreateOrUpdateOp {
    pub action: WriteOpAction,
    pub collection: String,
    pub rkey: String,
    pub record: RepoRecord,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RecordDeleteOp {
    pub action: WriteOpAction,
    pub collection: String,
    pub rkey: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum RecordWriteOp {
    Create(RecordCreateOrUpdateOp),
    Update(RecordCreateOrUpdateOp),
    Delete(RecordDeleteOp),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RecordCreateOrDeleteDescript {
    pub action: WriteOpAction,
    pub collection: String,
    pub rkey: String,
    pub cid: Cid,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RecordUpdateDescript {
    pub action: WriteOpAction,
    pub collection: String,
    pub rkey: String,
    pub prev: Cid,
    pub cid: Cid,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum RecordWriteDescript {
    Create(RecordCreateOrDeleteDescript),
    Update(RecordUpdateDescript),
    Delete(RecordCreateOrDeleteDescript),
}

pub type WriteLog = Vec<Vec<RecordWriteDescript>>;

// Updates/Commits
// ---------------

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct CommitData {
    pub cid: Cid,
    pub rev: String,
    pub since: Option<String>,
    pub prev: Option<Cid>,
    pub new_blocks: BlockMap,
    pub removed_cids: CidSet,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RepoUpdate {
    pub cid: Cid,
    pub rev: String,
    pub since: Option<String>,
    pub prev: Option<Cid>,
    pub new_blocks: BlockMap,
    pub removed_cids: CidSet,
    pub ops: Vec<RecordWriteOp>,
}

pub type CollectionContents = BTreeMap<String, RepoRecord>;
pub type RepoContents = BTreeMap<String, CollectionContents>;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RepoRecordWithCid {
    pub cid: Cid,
    pub value: RepoRecord,
}
pub type CollectionContentsWithCids = BTreeMap<String, RepoRecordWithCid>;
pub type RepoContentsWithCids = BTreeMap<String, CollectionContentsWithCids>;

pub type DatastoreContents = BTreeMap<String, Cid>;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RecordPath {
    pub collection: String,
    pub rkey: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RecordClaim {
    pub collection: String,
    pub rkey: String,
    pub record: Option<RepoRecord>,
}

// Sync
// ---------------

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct VerifiedDiff {
    pub write: Vec<RecordWriteDescript>,
    pub commit: CommitData,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct VerifiedRepo {
    pub creates: Vec<RecordCreateOrDeleteDescript>,
    pub commit: CommitData,
}

pub type CarBlock = CidAndBytes;
