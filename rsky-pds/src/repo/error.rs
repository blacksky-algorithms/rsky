use lexicon_cid::Cid;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DataStoreError {
    #[error("missing block `{0}`")]
    MissingBlock(String),
    #[error("missing `{0}` blocks: `{1:?}`")]
    MissingBlocks(String, Vec<Cid>),
    #[error("unexpected object at `{0}`")]
    UnexpectedObject(Cid),
    #[error("unknown data store error")]
    Unknown,
}

#[derive(Error, Debug)]
pub enum RepoError {
    #[error("Commit was at`{0}`")]
    BadCommitSwapError(Cid),
    #[error("Record was at`{0:?}`")]
    BadRecordSwapError(Option<Cid>),
    #[error("Invalid record error")]
    InvalidRecordError,
}

#[derive(Error, Debug)]
pub enum BlobError {
    #[error("Blob not found")]
    BlobNotFoundError,
}
